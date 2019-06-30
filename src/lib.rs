//! Utilities for creating always present thread locals.
//!
//! The `phoenix_tls!` macro creates a `thread_local!` style variable that is lazily initialized. If
//! the `thread_local` is accessed after it's destroyed, a new temporary will be created using
//! `Default::default()`.
//!
//! All phoenix thread locals (Phoenix) are internally reference counted heap allocated structures.
//!
//! Additionally the user type receives two callbacks `subscribe`/`unsubscribe`, which are invoked
//! at creation/desctruction. The address is stable between those two calls.

#![cfg_attr(not(feature = "std"), no_std)]

cfg_if::cfg_if! {
    if #[cfg(not(feature = "std"))] {
        extern crate alloc;
        use alloc::boxed::Box;
    }
}

use core::{cell::Cell, marker::PhantomData, mem::ManuallyDrop, ops::Deref, ptr::NonNull};

/// Types that can be stored in phoenix_tls's can implement this for optional callback hooks for
/// when they are created/destroyed.
pub trait PhoenixTarget: Default {
    /// Called when a phoenix `Self` is created.
    ///
    /// A `Self` lives at the address passed into subscribe at least until `unsubscribe` is called.
    fn subscribe(&mut self);

    /// Called when a phoenix `Self` is about to be dropped (usually at thread exit).
    ///
    /// Called with an address that was previously passed into `subscribe`.
    fn unsubscribe(&mut self);
}

#[derive(Default)]
pub struct NoSubscribe<T: ?Sized>(pub T);
impl<T: ?Sized + Default> PhoenixTarget for NoSubscribe<T> {
    #[inline]
    fn subscribe(&mut self) {}

    #[inline]
    fn unsubscribe(&mut self) {}
}

impl<T: ?Sized> Deref for NoSubscribe<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
#[repr(C)]
struct PhoenixImpl<T> {
    value:     T,
    ref_count: Cell<usize>,
}

#[derive(Debug)]
pub struct Phoenix<T: 'static + PhoenixTarget> {
    raw:     NonNull<PhoenixImpl<T>>,
    phantom: PhantomData<PhoenixImpl<T>>,
}

impl<T: 'static + PhoenixTarget> Clone for Phoenix<T> {
    #[inline]
    fn clone(&self) -> Self {
        let count = self.as_ref().ref_count.get();
        debug_assert!(count > 0, "attempt to clone a deallocated `Phoenix`");

        let new_count = count.wrapping_add(1);
        self.as_ref().ref_count.set(new_count);

        // We must check for overflow because users can mem::forget(x.clone())
        // repeatedly.
        if nudge::unlikely(new_count == 0) {
            nudge::abort()
        }

        Phoenix {
            raw:     self.raw,
            phantom: PhantomData,
        }
    }
}

impl<T: 'static + PhoenixTarget> Drop for Phoenix<T> {
    #[inline]
    fn drop(&mut self) {
        let count = self.as_ref().ref_count.get();
        debug_assert!(count > 0, "double free on `Phoenix` attempted");
        self.as_ref().ref_count.set(count - 1);

        if nudge::unlikely(count == 1) {
            // this is safe as long as the reference counting logic is safe
            unsafe {
                dealloc::<_>(self.raw);
            }

            #[inline(never)]
            #[cold]
            unsafe extern "C" fn dealloc<T: 'static + PhoenixTarget>(
                this_ptr: NonNull<PhoenixImpl<T>>,
            ) {
                let mut this = Box::from_raw(this_ptr.as_ptr());

                this.value.unsubscribe();
            }
        }
    }
}

#[doc(hidden)]
impl<T: 'static + PhoenixTarget> Phoenix<T> {
    #[cold]
    pub fn new() -> Self {
        let mut phoenix = Box::new(PhoenixImpl {
            value:     T::default(),
            ref_count: Cell::new(1),
        });
        phoenix.value.subscribe();
        let raw = unsafe { NonNull::new_unchecked(Box::into_raw(phoenix)) };
        Phoenix {
            raw,
            phantom: PhantomData,
        }
    }

    #[inline]
    unsafe fn clone_raw(raw: NonNull<T>) -> Self {
        let result = ManuallyDrop::new(Phoenix {
            raw:     raw.cast::<PhoenixImpl<T>>(),
            phantom: PhantomData,
        });
        (*result).clone()
    }

    #[inline]
    fn as_ref(&self) -> &PhoenixImpl<T> {
        // this is safe as long as the reference counting logic is safe
        unsafe { self.raw.as_ref() }
    }
}

impl<T: 'static + PhoenixTarget> Deref for Phoenix<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.as_ref().value
    }
}

#[cold]
fn run_on_default<F, O, T>(f: F) -> O
where
    F: FnOnce(&T) -> O,
    T: Default + PhoenixTarget + 'static,
{
    f(&*Phoenix::<T>::new())
}

pub struct PhoenixKey<T: PhoenixTarget + 'static> {
    #[doc(hidden)]
    pub __get: &'static std::thread::LocalKey<Phoenix<T>>,
}

impl<T: PhoenixTarget + 'static> Clone for PhoenixKey<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: PhoenixTarget + 'static> Copy for PhoenixKey<T> {}

impl<T: PhoenixTarget + 'static> PhoenixKey<T> {
    #[inline]
    pub fn handle(self) -> Phoenix<T> {
        self.with(|x| unsafe { Phoenix::clone_raw(x.into()) })
    }

    #[inline]
    pub fn with<F: FnOnce(&T) -> O, O>(self, f: F) -> O {
        match self.__get.try_with(|x| NonNull::from(&**x)).ok() {
            Some(nn) => f(unsafe { nn.as_ref() }),
            None => run_on_default::<_, _, T>(f),
        }
    }
}

#[macro_export]
macro_rules! phoenix_tls {
    // empty (base case for the recursion)
    () => {};

    // process multiple declarations
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty; $($rest:tt)*) => (
        phoenix_tls!{
            $(#[$attr])* $vis static $name: $t
        }
        phoenix_tls!($($rest)*);
    );

    // handle a single declaration
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty) => (
        $(#[$attr])* $vis const $name: $crate::PhoenixKey<$t> = $crate::PhoenixKey {
            __get: {
                thread_local!{
                    $(#[$attr])* $vis static __SLOW: $crate::Phoenix<$t> =
                        $crate::Phoenix::new();
                }

                &__SLOW
            }
        };
    );
}
