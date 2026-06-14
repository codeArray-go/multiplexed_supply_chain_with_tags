use colored::*;
use std::io::{self, BufRead, Write};

pub fn input(prompt: &str) -> anyhow::Result<String> {
    print!("  {} ", prompt.bright_white());
    print!(": ");
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub fn input_or_default(prompt: &str, default: &str) -> anyhow::Result<String> {
    print!("  {} [{}]: ", prompt.bright_white(), default.bright_black());
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed)
    }
}

pub fn input_optional(prompt: &str) -> anyhow::Result<String> {
    print!("  {} (optional, Enter to skip): ", prompt.bright_white());
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub fn confirm(prompt: &str) -> anyhow::Result<bool> {
    print!("  {} [y/N]: ", prompt.bright_white());
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

pub fn select(prompt: &str, choices: &[&str]) -> anyhow::Result<usize> {
    println!("  {}", prompt.bright_white().bold());
    for (i, choice) in choices.iter().enumerate() {
        println!("    {}  {}", format!("[{}]", i + 1).cyan(), choice);
    }
    print!("  Enter number (1-{}): ", choices.len());
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;

    let n: usize = line.trim().parse().unwrap_or(0);
    if n >= 1 && n <= choices.len() {
        Ok(n - 1)
    } else {
        println!("  {} Invalid selection, defaulting to 1.", "!".yellow());
        Ok(0)
    }
}

pub fn input_u32(prompt: &str) -> anyhow::Result<u32> {
    loop {
        let s = input(prompt)?;
        match s.parse::<u32>() {
            Ok(n) => return Ok(n),
            Err(_) => println!("  {} Please enter a valid number.", "!".yellow()),
        }
    }
}

pub fn input_usize(prompt: &str) -> anyhow::Result<usize> {
    loop {
        let s = input(prompt)?;
        match s.parse::<usize>() {
            Ok(n) => return Ok(n),
            Err(_) => println!("  {} Please enter a valid number.", "!".yellow()),
        }
    }
}
