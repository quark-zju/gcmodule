use super::*;
use crate::debug;
use crate::Trace;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::spawn;

type List = ThreadedCc<Mutex<Vec<Box<dyn Trace + Send + Sync>>>>;

fn test_cross_thread_cycle(n: usize) {
    let list: Arc<Mutex<Vec<List>>> = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let space = Arc::new(ThreadedObjectSpace::default());
    assert_eq!(space.count_tracked(), 0);

    let spawn_thread = |thread_id| {
        let value = Mutex::new(Vec::new());
        let space = space.clone();
        let list = list.clone();
        spawn(move || {
            debug::NEXT_DEBUG_NAME.with(|n| n.set(thread_id));
            let this: List = space.create(value);
            let mut list = list.lock().unwrap();
            for other in list.iter_mut() {
                let cloned_other = other.clone();
                let cloned_this = this.clone();
                let this_ref = this.borrow();
                this_ref.lock().unwrap().push(Box::new(cloned_other));
                let other_ref = other.borrow();
                other_ref.lock().unwrap().push(Box::new(cloned_this));
            }
            list.push(this);
        })
    };

    let threads: Vec<_> = (0..n).map(|i| spawn_thread(i)).collect();
    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(space.count_tracked(), n);
    assert_eq!(space.collect_cycles(), 0);

    drop(list);
    assert_eq!(space.collect_cycles(), n);
}

#[test]
fn test_2_thread_cycle() {
    test_cross_thread_cycle(2);
}

#[test]
fn test_17_thread_cycle() {
    test_cross_thread_cycle(17);
}

#[test]
fn test_100_thread_cycle() {
    test_cross_thread_cycle(100);
}

fn test_racy_threads(
    thread_count: usize,
    iteration_count: usize,
    create_cycles_bits: u32,
    collect_cycles_bits: u32,
) {
    let space = Arc::new(ThreadedObjectSpace::default());
    let mut tx_list = Vec::with_capacity(thread_count);
    let mut rx_list = Vec::with_capacity(thread_count);
    for _ in 0..thread_count {
        let (tx, rx) = channel();
        tx_list.push(tx);
        rx_list.push(rx);
    }

    let threads: Vec<_> = rx_list
        .into_iter()
        .enumerate()
        .map(|(i, rx)| {
            let space = space.clone();
            let tx_list = tx_list.clone();
            spawn(move || {
                for k in 0..iteration_count {
                    {
                        debug::NEXT_DEBUG_NAME.with(|n| n.set((i + 1) * 1000 + k));
                        let value = Mutex::new(Vec::new());
                        let acc: List = space.create(value);
                        {
                            let acc_ref = acc.borrow();
                            let mut locked = acc_ref.lock().unwrap();
                            while let Ok(received) = rx.try_recv() {
                                locked.push(received);
                            }
                        }
                        if (create_cycles_bits >> i) & 1 == 1 {
                            for j in 0..thread_count {
                                if j % (i + 1) == 0 {
                                    let _ = tx_list[j].send(Box::new(acc.clone()));
                                }
                            }
                        }
                    }

                    if (collect_cycles_bits >> i) & 1 == 1 {
                        space.collect_cycles();
                    }
                }
            })
        })
        .collect();

    for t in threads {
        t.join().unwrap();
    }

    space.collect_cycles();
    assert_eq!(space.count_tracked(), 0);
}

#[test]
fn test_racy_threads_drops() {
    test_racy_threads(32, 1000, 0, 0);
}

#[test]
fn test_racy_threads_collects() {
    test_racy_threads(32, 20, 0xffffffff, 0xffffffff);
}

#[test]
fn test_racy_threads_mixed_collects() {
    test_racy_threads(8, 100, 0b11110000, 0b10101010);
}
