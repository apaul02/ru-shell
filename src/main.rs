use std::env;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Completer, Editor, Helper, Highlighter, Hinter, Validator};

use nix::sys::signal::{SigHandler, Signal, signal};
use nix::unistd::{Pid, getpgrp, tcsetpgrp};

#[derive(Helper, Completer, Hinter, Highlighter, Validator)]
struct ShellHelper {
    #[rustyline(Completer)]
    completer: FilenameCompleter,
}

fn main() {
    unsafe {
        signal(Signal::SIGTTOU, SigHandler::SigIgn).expect("Failed to ignore SIGTTOU");
    }
    let helper = ShellHelper {
        completer: FilenameCompleter::new(),
    };
    let mut rl = Editor::<ShellHelper, DefaultHistory>::new().expect("Failed to create editor");

    rl.set_helper(Some(helper));

    // 2. Load history if it exists
    let history_file = ".rusty_shell_history";
    if rl.load_history(history_file).is_err() {
        println!("No previous history found");
    }

    let mut background_jobs: Vec<Child> = Vec::new();

    loop {
        // Before we print the prompt, check if any background jobs finished.
        background_jobs.retain_mut(|child| {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("\n[Background job {} finished: {}]", child.id(), status);
                    false // Remove from the list
                }
                Ok(None) => true, // Still running
                Err(e) => {
                    eprintln!("\nError checking background jobs: {}", e);
                    false
                }
            }
        });
        let current_dir = env::current_dir().unwrap_or_default();
        let prompt = format!("{} $ ", current_dir.display());

        // Read the input using rustyline
        let readline = rl.readline(&prompt);

        match readline {
            Ok(line) => {
                let mut input = line.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                // Add the successful command to history
                let _ = rl.add_history_entry(input.clone());

                // detect background jobs
                let run_in_background = if input.ends_with('&') {
                    input.pop();
                    input = input.trim().to_string();
                    true
                } else {
                    false
                };

                let pipe_segments: Vec<&str> = input.split('|').collect();

                if pipe_segments.len() == 1 {
                    let mut parts = pipe_segments[0].split_whitespace();
                    let cmd = parts.next().unwrap();

                    if cmd == "exit" {
                        break;
                    } else if cmd == "cd" {
                        let new_dir = parts.peekable().peek().map_or("/", |x| *x);
                        let root = Path::new(new_dir);
                        if let Err(e) = env::set_current_dir(&root) {
                            eprintln!("cd Error: {}", e);
                        }
                        continue;
                    }
                }
                if let Some(bg_child) = execute_pipeline(pipe_segments, run_in_background) {
                    background_jobs.push(bg_child);
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Handle Ctrl + C
                println!("^C");
            }
            Err(ReadlineError::Eof) => {
                // Ctri + D
                println!("exit");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    if let Err(err) = rl.save_history(history_file) {
        eprintln!("Failed to save histor: {}", err);
    }
}

fn execute_pipeline(segments: Vec<&str>, run_in_background: bool) -> Option<Child> {
    let mut previous_command: Option<Child> = None;
    let num_segments = segments.len();

    // Track the Process Group ID (PGID) for the pipeline
    let mut pipeline_pgid: i32 = 0;
    let terminal_stdin = std::io::stdin();

    for (i, segment) in segments.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == num_segments - 1;

        // 1. Parse the command and look for redirection (<, >)
        let mut args = segment.split_whitespace();
        let cmd = match args.next() {
            Some(c) => c,
            None => {
                eprintln!("Parse error: missing command");
                return None;
            }
        };
        let mut final_args = Vec::new();

        let mut input_file = None;
        let mut output_file = None;

        while let Some(arg) = args.next() {
            match arg {
                "<" => input_file = args.next(), // Grab the next token as the filename
                ">" => output_file = args.next(),
                _ => final_args.push(arg),
            }
        }

        // 2. Setup Stdin
        let stdin = if let Some(filename) = input_file {
            // Redirect from file
            match File::open(filename) {
                Ok(file) => Stdio::from(file),
                Err(e) => {
                    eprintln!("Error opening input file: {}", e);
                    return None;
                }
            }
        } else if !is_first {
            // Take the previous child's stdout and use it as this child's stdin
            let prev_stdout = previous_command.as_mut().unwrap().stdout.take().unwrap();
            Stdio::from(prev_stdout)
        } else {
            Stdio::inherit()
        };

        // 3. Setup Stdout
        let stdout = if let Some(filename) = output_file {
            // Redirect to file
            match File::create(filename) {
                Ok(file) => Stdio::from(file),
                Err(e) => {
                    eprintln!("Error creating output file: {}", e);
                    return None;
                }
            }
        } else if !is_last {
            // Pipe to the next command
            Stdio::piped()
        } else {
            Stdio::inherit()
        };

        let mut cmd_builder = Command::new(cmd);
        cmd_builder.args(final_args).stdin(stdin).stdout(stdout);

        // Process Group Logic
        // If its the first command, process_group(0) creates a new Group.
        // For Subsequent command in the pipe, we put them in the same Group as the first
        cmd_builder.process_group(pipeline_pgid);

        let child = cmd_builder.spawn();

        // 4. Spawn the process

        match child {
            Ok(child) => {
                if is_first {
                    // Gran the PID of the first child, this is our new Process Group ID.
                    pipeline_pgid = child.id() as i32;
                    let pgid = Pid::from_raw(pipeline_pgid);

                    // Handle the terminal over the child's process Group
                    if !run_in_background {
                        if let Err(e) = tcsetpgrp(&terminal_stdin, pgid) {
                            eprintln!("Failed to give terminal to child: {}", e);
                        }
                    }
                }
                previous_command = Some(child);
            }
            Err(e) => {
                eprintln!("Command not found or error: {}", e);
                return None;
            }
        }
    }

    if run_in_background {
        if let Some(child) = &previous_command {
            println!("[Started background job. PID: {}]", child.id());
        }
        return previous_command;
    } else {
        if let Some(mut final_child) = previous_command {
            if let Err(e) = final_child.wait() {
                eprintln!("Error waiting on pipeline: {}", e);
            }
        }
    }

    // Take the terminal back
    let shell_pgid = getpgrp();
    if let Err(e) = tcsetpgrp(&terminal_stdin, shell_pgid) {
        eprintln!("Failed to take back the terminal: {}", e);
    }
    return None;
}
