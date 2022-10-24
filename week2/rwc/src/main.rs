use std::env;
use std::fs::File;
use std::io::{self, BufRead};
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Too few arguments.");
        process::exit(1);
    }
    let filename = &args[1];

    // Your code here :)

    let file = File::open(filename).expect("Invalid file name");
    let mut lines_cnt = 0;
    let mut letters_cnt = 0;
    let mut words_cnt = 0;
    for line in io::BufReader::new(file).lines() {
        let line_str = line.unwrap();
        println!("lines: {:?}", line_str);
        lines_cnt += 1;
        for letter in line_str.chars().map(|c| c.to_string()) {
            if letter == " " {
                words_cnt += 1;
            } else {
                letters_cnt += 1;
            }
        }
        words_cnt += 1;
    }
    println!("lines: {lines_cnt}, words: {words_cnt}, letters: {letters_cnt}");
}
