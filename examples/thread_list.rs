use phoenix_tls::{phoenix_tls, PhoenixTarget};
use std::sync::{
    atomic::{AtomicUsize, Ordering::Relaxed},
    Mutex,
};

lazy_static::lazy_static! {
    static ref THREAD_LIST: Mutex<Vec<usize>> = Default::default();
}

static THREAD_ID: AtomicUsize = AtomicUsize::new(0);

struct Thread {
    id: usize,
}

impl Default for Thread {
    fn default() -> Self {
        let id = THREAD_ID.fetch_add(1, Relaxed);
        Self { id }
    }
}

impl PhoenixTarget for Thread {
    fn subscribe(&mut self) {
        THREAD_LIST.lock().unwrap().push(self as *const _ as usize)
    }

    fn unsubscribe(&mut self) {
        let mut list = THREAD_LIST.lock().unwrap();
        let position = list
            .iter()
            .position(|x| *x == self as *const _ as usize)
            .unwrap();
        list.remove(position);
    }
}

phoenix_tls! {
    static THREAD: Thread;
}

fn main() {
    let main_id = THREAD.handle().id;
    println!("main id: {}", main_id);

    assert_eq!(main_id, THREAD.handle().id);
    std::thread::spawn(move || {
        println!("num threads: {}", THREAD_LIST.lock().unwrap().len());
        let this_id = THREAD.handle().id;
        println!("other id: {}", this_id);
        println!("num threads: {}", THREAD_LIST.lock().unwrap().len());
        assert_ne!(main_id, this_id);
        assert_eq!(this_id, THREAD.handle().id);
    })
    .join()
    .unwrap();
    println!("num threads: {}", THREAD_LIST.lock().unwrap().len())
}
