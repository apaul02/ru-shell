use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

fn main() {
    loop {
        let current_dir = env::current_dir().unwrap_or_default();
        print!("{} $ ", current_dir.display());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).unwrap() == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Split the input into pipeline segments based on '|'
        let pipe_segments: Vec<&str> = input.split('|').collect();

        // Handle built-ins (we'll only handle 'cd' and 'exit' if they are the ONLY command)
        if pipe_segments.len() == 1 {
            let mut parts = pipe_segments[0].split_whitespace();
            let cmd = parts.next().unwrap();

            if cmd == "exit" {
                break;
            } else if cmd == "cd" {
                let new_dir = parts.peekable().peek().map_or("/", |x| *x);
                let root = Path::new(new_dir);
                if let Err(e) = env::set_current_dir(&root) {
                    eprintln!("cd error: {}", e);
                }
                continue;
            }
        }

        execute_pipeline(pipe_segments);
    }
}

fn execute_pipeline(segments: Vec<&str>) {
    let mut previous_command: Option<Child> = None;
    let num_segments = segments.len();

    for (i, segment) in segments.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == num_segments - 1;

        // 1. Parse the command and look for redirection (<, >)
        let mut args = segment.split_whitespace();
        let cmd = args.next().unwrap();
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
                    return;
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
                    return;
                }
            }
        } else if !is_last {
            // Pipe to the next command
            Stdio::piped()
        } else {
            Stdio::inherit()
        };

        // 4. Spawn the process
        let child = Command::new(cmd)
            .args(final_args)
            .stdin(stdin)
            .stdout(stdout)
            .spawn();

        match child {
            Ok(child) => {
                previous_command = Some(child);
            }
            Err(e) => {
                eprintln!("Command not found or error: {}", e);
                return; // Abort the rest of the pipeline
            }
        }
    }

    // 5. Wait for the last command in the pipeline to finish
    if let Some(mut final_child) = previous_command {
        if let Err(e) = final_child.wait() {
            eprintln!("Error waiting on pipeline: {}", e);
        }
    }
}
