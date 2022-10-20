// Simple Hangman Program
// User gets five incorrect guesses
// Word chosen randomly from words.txt
// Inspiration from: https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
// This assignment will introduce you to some fundamental syntax in Rust:
// - variable declaration
// - string manipulation
// - conditional statements
// - loops
// - vectors
// - files
// - user input
// We've tried to limit/hide Rust's quirks since we'll discuss those details
// more in depth in the coming lectures.
extern crate rand;
use rand::Rng;
use std::fs;
use std::io;
use std::io::Write;

const NUM_INCORRECT_GUESSES: u32 = 5;
const WORDS_PATH: &str = "words.txt";

fn pick_a_random_word() -> String {
    let file_string = fs::read_to_string(WORDS_PATH).expect("Unable to read file.");
    let words: Vec<&str> = file_string.split('\n').collect();
    String::from(words[rand::thread_rng().gen_range(0, words.len())].trim())
}

fn main() {
    let secret_word = pick_a_random_word();
    // Note: given what you know about Rust so far, it's easier to pull characters out of a
    // vector than it is to pull them out of a string. You can get the ith character of
    // secret_word by doing secret_word_chars[i].
    let secret_word_chars: Vec<char> = secret_word.chars().collect();
    // Uncomment for debugging:
    println!("random word: {}", secret_word);
    // println!("{}", mem::size_of_val(&secret_word_chars));

    let mut guessed_letters: Vec<char> = Vec::new();
    let mut word_so_far: Vec<char> = String::from("----------------")[0..secret_word_chars.len()]
        .chars()
        .collect();
    // Your code here! :)
    let mut match_letters_cnt = 0;
    let mut already_try: u32 = 0;
    loop {
        if already_try == NUM_INCORRECT_GUESSES {
            println!("Sorry, you ran out of guesses!");
            break;
        }

        println!("The word so far is {:?}", word_so_far);
        println!(
            "You have guessed the following letters: {:?}",
            guessed_letters
        );
        print!(
            "You have {} guesses left\n",
            secret_word_chars.len() - match_letters_cnt
        );
        print!("Please guess a letter: ");
        // Make sure the prompt from the previous line gets displayed;
        io::stdout().flush().expect("Error flushing stdout.");
        let mut guess = String::new();
        io::stdin()
            .read_line(&mut guess)
            .expect("Error reading line.");
        let guess_letters: Vec<char> = guess.chars().collect();
        guessed_letters.push(guess_letters[0]);

        let mut flag: bool = false;
        for idx in 0..secret_word_chars.len() {
            if guess_letters[0] == secret_word_chars[idx] && word_so_far[idx] == '-' {
                match_letters_cnt += 1;
                word_so_far[idx] = guess_letters[0];
                flag = true;
                break;
            }
        }
        if !flag {
            println!("Sorry, that letter is not in the word");
        }
        println!("\n");

        if match_letters_cnt == secret_word_chars.len() {
            println!("Congratulations you guessed the secret word: {secret_word}!");
            break;
        }
        already_try += 1;
    }
}
