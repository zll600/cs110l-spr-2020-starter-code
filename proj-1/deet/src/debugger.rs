use crate::debugger_command::DebuggerCommand;
use crate::dwarf_data::{DwarfData, Error as DwarfError};
use crate::inferior::{Breakpoint, Inferior, Status};
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::collections::HashMap;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    inferior: Option<Inferior>,
    debug_data: DwarfData,
    // breakpoints: Vec<usize>,
    breakpoints: HashMap<usize, Breakpoint>,
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        // TODO (milestone 3): initialize the DwarfData

        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);

        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Couldn't not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };
        debug_data.print();

        let breakpoints: HashMap<usize, Breakpoint> = HashMap::new();
        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
            debug_data,
            breakpoints,
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Run(args) => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().kill();
                        self.inferior = None;
                    }
                    if let Some(inferior) =
                        Inferior::new(&self.target, &args, &mut self.breakpoints)
                    {
                        // Create the inferior
                        self.inferior = Some(inferior);
                        // TODO (milestone 1): make the inferior run
                        // You may use self.inferior.as_mut().unwrap() to get a mutable reference
                        // to the Inferior object
                        match self
                            .inferior
                            .as_mut()
                            .unwrap()
                            .continue_run(None, &self.breakpoints)
                            .unwrap()
                        {
                            Status::Exited(exit_code) => {
                                println!("Child exited (status {})", exit_code)
                            }
                            Status::Signaled(signal) => println!("Child exited due to {}", signal),
                            Status::Stopped(signal, rip) => {
                                println!(
                                    "Child stopped by signal {} at address {:#x}",
                                    signal, rip
                                );
                                let dwarf_line = self.debug_data.get_line_from_addr(rip).unwrap();
                                println!("Stopped at ({})", dwarf_line);
                            }
                        }
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                DebuggerCommand::Quit => {
                    if self.inferior.is_some() {
                        self.inferior.as_mut().unwrap().kill();
                        self.inferior = None;
                    }
                    return;
                }
                DebuggerCommand::Continue => {
                    if self.inferior.is_none() {
                        println!("There is not one running!");
                        continue;
                    }
                    match self
                        .inferior
                        .as_mut()
                        .unwrap()
                        .continue_run(None, &self.breakpoints)
                        .unwrap()
                    {
                        Status::Exited(exit_code) => {
                            println!("Child exited (status {})", exit_code);
                            self.inferior = None;
                        }
                        Status::Signaled(signal) => {
                            println!("Child exited due to {}", signal);
                            self.inferior = None;
                        }
                        Status::Stopped(signal, rip) => {
                            println!("Child stopped by signal {} at address {:#x}", signal, rip)
                        }
                    }
                }
                DebuggerCommand::Backtrace => {
                    if self.inferior.is_some() {
                        self.inferior
                            .as_mut()
                            .unwrap()
                            .print_backtrace(&self.debug_data)
                            .unwrap();
                    } else {
                        println!("Error No process is running, you can not use backtrace command!");
                    }
                }
                DebuggerCommand::BreakPoint(address) => {
                    if !address.starts_with("*") {
                        println!("Usage: breakpoint *address!");
                        continue;
                    }
                    if let Some(addr) = self.parse_address(&address[1..]) {
                        if self.inferior.is_some() {
                            if let Ok(orig_byte) =
                                self.inferior.as_mut().unwrap().write_byte(addr, 0xcc)
                            {
                                println!(
                                    "Set breakpoint {} at {}",
                                    self.breakpoints.len(),
                                    address
                                );
                                self.breakpoints
                                    .insert(addr, Breakpoint { addr, orig_byte });
                            } else {
                                println!(
                                    "Error in Setting breakpoint at invalid address {:#x}",
                                    addr
                                );
                            }
                        } else {
                            println!("Set breakpoint {} at {}", self.breakpoints.len(), address);
                            self.breakpoints
                                .insert(addr, Breakpoint { addr, orig_byte: 0 });
                        }
                    } else {
                        println!(
                            "Error in Setting breakpoint at invalid address: {}",
                            address
                        );
                    }
                }
            }
        }
    }

    /// This function prompts the user to enter a command, and continues re-prompting until the user
    /// enters a valid command. It uses DebuggerCommand::from_tokens to do the command parsing.
    ///
    /// You don't need to read, understand, or modify this function.
    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            // Print prompt and get next line of user input
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    // User pressed ctrl+c. We're going to ignore it
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    // User pressed ctrl+d, which is the equivalent of "quit" for our purposes
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().len() == 0 {
                        continue;
                    }
                    self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }

    fn parse_address(&self, addr: &str) -> Option<usize> {
        // let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
        //     &addr[2..]
        // } else {
        //     &addr
        // };
        // usize::from_str_radix(addr_without_0x, 16).ok()
        if addr.to_lowercase().starts_with("0x") {
            usize::from_str_radix(&addr[2..], 16).ok()
        } else if let Ok(line) = addr.parse::<usize>() {
            self.debug_data.get_addr_for_line(None, line)
        } else if let Some(address) = self.debug_data.get_addr_for_function(None, addr) {
            Some(address)
        } else {
            usize::from_str_radix(&addr, 16).ok()
        }
    }
}
