use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;

pub fn ls(path: &str) -> io::Result<()> {
    let entries = fs::read_dir(path)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        
        // ドットファイル（隠しファイル）を除外 (C版 horiz-ls.c の挙動に準拠)
        if name_str.starts_with('.') {
            continue;
        }
        
        print!("{}  ", name_str);
    }
    println!();
    Ok(())
}

pub fn cat(files: Vec<String>) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let mut buffer = [0; 1024];

    for file in files {
        let mut f = fs::File::open(file)?;
        loop {
            let n = f.read(&mut buffer)?;
            if n == 0 { break; }
            handle.write_all(&buffer[..n])?;
        }
    }
    handle.flush()?;
    Ok(())
}

pub fn echo(args: Vec<String>) {
    println!("{}", args.join(" "));
}

/// パスを正規化してディレクトリトラバーサルを防ぐ (Zero-Dependency)
pub fn chmod(args: Vec<String>) -> io::Result<()> {
    if args.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Usage: chmod <octal_mode> <file1> [<file2> ...]"));
    }

    let mode_str = &args[0];
    let mode = u32::from_str_radix(mode_str, 8).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid octal mode '{}'", mode_str))
    })?;

    for file_path in args.iter().skip(1) {
        let mut permissions = fs::metadata(file_path)?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(file_path, permissions)?;
    }

    Ok(())
}

/// パスを正規化してディレクトリトラバーサルを防ぐ (Zero-Dependency)
pub fn normalize_path(path: &str) -> String {
    let mut stack = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                stack.pop();
            }
            _ => {
                stack.push(component);
            }
        }
    }
    format!("/{}", stack.join("/"))
}

