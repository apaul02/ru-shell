# 🦀 RustyShell

A fully functional, UNIX-compliant shell written entirely in Rust. 

RustyShell was built from scratch to explore low-level operating system primitives, bridging the gap between Rust's strict safety guarantees and raw POSIX system calls. It features a custom lexical analyzer, full job control, pipeline management, and a robust interactive terminal UI.



## ✨ Features

* **Interactive REPL:** Powered by `rustyline` for a true terminal experience, including persistent history, arrow-key navigation, and tab-completion for file paths.
* **Custom Lexical Analyzer:** A smart, character-by-character tokenizer that correctly parses quotes (`"`, `'`) and operators (`|`, `>`, `<`, `&`) even without surrounding spaces.
* **Process Execution:** Safe wrappers around `fork` and `exec` system calls to spawn child processes.
* **Pipelines (`|`):** Connects multiple commands using in-memory anonymous OS pipes (e.g., `ls -l | grep Cargo | sort`).
* **I/O Redirection (`<`, `>`):** Seamlessly redirects Standard Input and Standard Output to and from files using raw file descriptors.
* **Job Control:** * **Foreground Isolation:** Uses `tcsetpgrp` to assign distinct Process Groups to pipelines, ensuring `Ctrl+C` (`SIGINT`) only kills the running command, not the shell itself.
  * **Background Jobs (`&`):** Executes commands in the background without blocking the terminal, complete with an automated zombie-process reaper.
* **Expansions:** Automatically expands the Tilde (`~`) to the user's home directory and substitutes Environment Variables (e.g., `$USER`).
* **Built-in Commands:** Handles `cd`, `export`, and `exit` internally without spawning separate processes.

## 🚀 Getting Started

### Prerequisites
You will need [Rust and Cargo](https://rustup.rs/) installed on your machine. This shell is designed for UNIX-like systems (Linux, macOS) due to its heavy reliance on POSIX signals and terminal control.

### Installation
Clone the repository and build the project using Cargo:

```bash
git clone [https://github.com/yourusername/rusty_shell.git](https://github.com/yourusername/rusty_shell.git)
cd rusty_shell
cargo build --release```

### Running the shell
Execute the compiled binary
```bash
cargo run 
# or 
./target/release/rusty_shell
```
### 💻 Usage Examples

#### Basic Execution and Built-in
```bash
/home/user $ echo "Hello, World!"
/home/user $ cd ~/Documents
/home/user/Documents $ export MY_VAR=Rust
/home/user/Documents $ echo $MY_VAR
```
#### Piping and Redirection
```bash
/home/user $ ls-la|grep "txt">output.txt
/home/user $ cat < output.txt
```

#### Background Jobs
```bash
/home/user $ sleep 5 &
[Started background job. PID: 12345]
/home/user $ echo "I can keep typing!"
I can keep typing!

[Background job 12345 finished: exit status: 0]
```

## 🛠️ Architecture & Core Dependencies

- nix: Used for safe, Rust-friendly bindings to libc. Critical for managing Process Groups (getpgrp, tcsetpgrp), signal handling (SIGTTOU, SIGINT), and PID tracking.

- rustyline: A Readline implementation for Rust. Provides the terminal UI, history file management, and FilenameCompleter.

- dirs: A lightweight library for safely resolving the user's home directory across different platforms.
