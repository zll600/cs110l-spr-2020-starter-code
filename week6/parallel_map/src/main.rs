use crossbeam_channel;
use std::{thread, time};

fn parallel_map<T, U, F>(mut input_vec: Vec<T>, num_threads: usize, f: F) -> Vec<U>
where
    F: FnOnce(T) -> U + Send + Copy + 'static,
    T: Send + 'static,
    U: Send + 'static + Default,
{
    let mut output_vec: Vec<U> = Vec::with_capacity(input_vec.len());
    output_vec.resize_with(input_vec.len(), Default::default);
    // TODO: implement parallel map!
    let (sender, receiver) = crossbeam_channel::unbounded();
    let (out_sender, out_receiver) = crossbeam_channel::unbounded();

    let mut threads = Vec::new();
    for _ in 0..num_threads {
        let receiver_clone = receiver.clone();
        let out_sender_clone = out_sender.clone();
        threads.push(thread::spawn(move || {
            while let Ok(pair) = receiver_clone.recv() {
                let (val, idx) = pair;
                out_sender_clone
                    .send((f(val), idx))
                    .expect("Tried writint to channel, but there are no out_receivers!");
            }
        }));
    }

    let len = input_vec.len();
    for i in 0..len {
        sender
            .send((input_vec.pop().unwrap(), len - i - 1))
            .expect("Tried writing to channel, but there are no receivers!");
    }
    drop(sender);
    drop(out_sender);

    while let Ok(pair) = out_receiver.recv() {
        let (val, idx) = pair;
        output_vec[idx] = val;
    }

    for thread in threads {
        thread.join().expect("Panic occured in thread");
    }

    output_vec
}

fn main() {
    let v = vec![6, 7, 8, 9, 10, 1, 2, 3, 4, 5, 12, 18, 11, 5, 20];
    let squares = parallel_map(v, 10, |num| {
        println!("{} squared is {}", num, num * num);
        thread::sleep(time::Duration::from_millis(500));
        num * num
    });
    println!("squares: {:?}", squares);
}
