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

                let mut tokens = tokenize(&input);
                if tokens.is_empty() {
                    continue;
                }

                // detect background jobs
                let run_in_background = if input.ends_with('&') {
                    input.pop();
                    input = input.trim().to_string();
                    true
                } else {
                    false
                };

                let mut pipe_segments: Vec<Vec<String>> = Vec::new();
                let mut current_segment = Vec::new();

                for token in tokens {
                    if token == "|" {
                        pipe_segments.push(current_segment);
                        current_segment = Vec::new();
                    } else {
                        current_segment.push(token);
                    }
                }

                pipe_segments.push(current_segment);

                if pipe_segments.len() == 1 && !pipe_segments[0].is_empty() {
                    let cmd = &pipe_segments[0][0];
                    let args = &pipe_segments[0][1..];

                    if cmd == "exit" {
                        break;
                    } else if cmd == "cd" {
                        let new_dir = args.first().map_or("/", |x| x.as_str());
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

fn execute_pipeline(segments: Vec<Vec<String>>, run_in_background: bool) -> Option<Child> {
    let mut previous_command: Option<Child> = None;
    let num_segments = segments.len();

    // Track the Process Group ID (PGID) for the pipeline
    let mut pipeline_pgid: i32 = 0;
    let terminal_stdin = std::io::stdin();

    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            eprintln!("Parse error: Empty pipeline segment");
        }
        let is_first = i == 0;
        let is_last = i == num_segments - 1;

        let mut args = segment.iter();
        let cmd = args.next().unwrap();
        let mut final_args = Vec::new();

        let mut input_file = None;
        let mut output_file = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "<" => input_file = args.next().map(|s| s.clone()),
                ">" => output_file = args.next().map(|s| s.clone()),
                _ => final_args.push(arg.clone()),
            }
        }

        let stdin = if let Some(filename) = input_file {
            match File::open(&filename) {
                Ok(file) => Stdio::from(file),
                Err(e) => {
                    eprintln!("Error opening input file {}: {}", filename, e);
                    return None;
                }
            }
        } else if !is_first {
            let prev_stdout = previous_command.as_mut().unwrap().stdout.take().unwrap();
            Stdio::from(prev_stdout)
        } else {
            Stdio::inherit()
        };

        let stdout = if let Some(filename) = output_file {
            match File::create(&filename) {
                Ok(file) => Stdio::from(file),
                Err(e) => {
                    eprintln!("Error creating output file {}: {}", filename, e);
                    return None;
                }
            }
        } else if !is_last {
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

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let mut current_token = String::new();
    let mut in_quotes: Option<char> = None; // Tracks if we are inside " or '

    while let Some(&c) = chars.peek() {
        match c {
            // 1. Handle Quotes
            '"' | '\'' => {
                if let Some(q) = in_quotes {
                    if q == c {
                        in_quotes = None; // Close quote
                    } else {
                        current_token.push(c); // It's the other type of quote, treat as text
                    }
                } else {
                    in_quotes = Some(c); // Open quote
                }
                chars.next();
            }
            // 2. Handle Whitespace (if not in quotes)
            _ if c.is_whitespace() && in_quotes.is_none() => {
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
                chars.next();
            }
            // 3. Handle Operators (if not in quotes)
            '|' | '<' | '>' | '&' if in_quotes.is_none() => {
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
                tokens.push(c.to_string());
                chars.next();
            }
            // 4. Normal characters
            _ => {
                current_token.push(c);
                chars.next();
            }
        }
    }

    if !current_token.is_empty() {
        tokens.push(current_token);
    }
    tokens
}
