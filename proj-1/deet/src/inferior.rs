use crate::dwarf_data::DwarfData;
use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::mem::size_of;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub enum Status {
    /// Indicates inferior stopped. Contains the signal that stopped the process, as well as the
    /// current instruction pointer that it is stopped at.
    Stopped(signal::Signal, usize),

    /// Indicates inferior exited normally. Contains the exit status code.
    Exited(i32),

    /// Indicates the inferior exited due to a signal. Contains the signal that killed the
    /// process.
    Signaled(signal::Signal),
}

/// This function calls ptrace with PTRACE_TRACEME to enable debugging on a process. You should use
/// pre_exec with Command to call this in the child process.
fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

#[derive(Clone)]
pub struct Breakpoint {
    pub addr: usize,
    pub orig_byte: u8,
}

pub struct Inferior {
    child: Child,
}

impl Inferior {
    /// Attempts to start a new inferior process. Returns Some(Inferior) if successful, or None if
    /// an error is encountered.
    pub fn new(
        target: &str,
        args: &Vec<String>,
        breakpoints: &mut HashMap<usize, Breakpoint>,
    ) -> Option<Inferior> {
        // TODO: implement me!
        let mut cmd = Command::new(target);
        cmd.args(args);
        unsafe {
            cmd.pre_exec(child_traceme);
        }
        let child = cmd.spawn().expect("Error in Inferiro::new");
        let mut inferior = Inferior { child };
        /*
        match inferior.wait(None).ok()? {
            Status::Exited(exit_code) => println!("Child exited (status {})", exit_code),
            Status::Signaled(signal) => println!("Child exited due to {}", signal),
            Status::Stopped(signal, rip) => {
                println!("Child stopped by signal {} at address {:#x}", signal, rip)
            }
        }
        */
        let breakpoints_clone = breakpoints.clone();
        for bp in breakpoints_clone.keys() {
            match inferior.write_byte(*bp, 0xcc) {
                Ok(orig_byte) => {
                    breakpoints
                        .insert(
                            *bp,
                            Breakpoint {
                                addr: *bp,
                                orig_byte,
                            },
                        )
                        .unwrap();
                }
                Err(_) => println!("Error address is invalid: {:#x}", *bp),
            }
        }
        Some(inferior)
    }

    /// Returns the pid of this inferior.
    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    /// Calls waitpid on this inferior and returns a Status to indicate the state of the process
    /// after the waitpid call.
    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    pub fn continue_run(
        &mut self,
        sig: Option<signal::Signal>,
        breakpoints: &HashMap<usize, Breakpoint>,
    ) -> Result<Status, nix::Error> {
        let mut regs = ptrace::getregs(self.pid())?;
        let rip = regs.rip as usize;

        // if inferior is stopped at a breakpoint(i.e. (%rip - 1) matches a breakpoint address.)
        if let Some(breakpoint) = breakpoints.get(&(rip - 1)) {
            println!("Stop at a breakpoint!");
            // restore the first byte of the instruction we replaced
            self.write_byte(rip - 1, breakpoint.orig_byte).unwrap();
            // set %rip = %rip - 1 to rewind the instruction pointer
            regs.rip = (rip - 1) as u64;
            ptrace::setregs(self.pid(), regs)?;
            // ptrace::stop to go to next breakpoint
            ptrace::step(self.pid(), None)?;
            // wait for inferior to stop due to SIGTRAP
            match self.wait(None).ok().unwrap() {
                Status::Exited(exit_code) => return Ok(Status::Exited(exit_code)),
                Status::Signaled(signal) => return Ok(Status::Signaled(signal)),
                Status::Stopped(_, _) => self.write_byte(rip - 1, 0xcc).unwrap(),
            };
        }
        // ptrace::cont to resume normal executation
        ptrace::cont(self.pid(), sig)?;
        // wait for inferior to stop or terminate
        self.wait(None)
    }

    pub fn kill(&mut self) {
        self.child.kill().unwrap();
        self.wait(None).unwrap();
        println!("Killing running inferior (pid: {})", self.pid());
    }

    pub fn print_backtrace(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid())?;

        let mut rip = regs.rip as usize;
        let mut rbp = regs.rbp as usize;

        loop {
            let dwarf_line = debug_data.get_line_from_addr(rip);
            let dwarf_func = debug_data.get_function_from_addr(rip);
            match (&dwarf_line, &dwarf_func) {
                (None, None) => {
                    println!("Unknown function name (Cannot find source file)");
                }
                (Some(line), None) => {
                    println!("Unknown function name ({})", line);
                }
                (None, Some(func)) => {
                    println!("{} (Cannot find source file)", func);
                }
                (Some(line), Some(func)) => {
                    println!("{} ({})", func, line);
                }
            }
            if let Some(func) = dwarf_func {
                if func == "main" {
                    break;
                }
            }

            rip = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType)? as usize;
            rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
        }
        Ok(())
    }

    pub fn write_byte(&mut self, addr: usize, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as *mut std::ffi::c_void,
        )?;
        Ok(orig_byte as u8)
    }
}

fn align_addr_to_word(addr: usize) -> usize {
    addr & (-(size_of::<usize>() as isize) as usize)
}
