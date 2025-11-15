use std::io;
use std::{fs::File, io::BufReader, path::PathBuf};

pub enum PathOrStdio {
    Path(PathBuf),
    Stdio,
}

impl From<String> for PathOrStdio {
    fn from(s: String) -> Self {
        if s == "-" {
            PathOrStdio::Stdio
        } else {
            PathOrStdio::Path(PathBuf::from(s))
        }
    }
}

impl PathOrStdio {
    pub fn filepath(&self) -> &str {
        match self {
            PathOrStdio::Path(p) => p.to_str().unwrap_or("input"),
            PathOrStdio::Stdio => "stdio",
        }
    }

    pub fn reader(&self) -> io::Result<Box<dyn io::Read>> {
        match self {
            PathOrStdio::Path(p) => {
                let file = File::open(p)?;
                Ok(Box::new(BufReader::new(file)))
            }
            PathOrStdio::Stdio => Ok(Box::new(io::stdin())),
        }
    }

    pub fn writer(&self) -> io::Result<Box<dyn io::Write>> {
        match self {
            PathOrStdio::Path(p) => {
                let file = File::create(p)?;
                Ok(Box::new(file))
            }
            PathOrStdio::Stdio => Ok(Box::new(io::stdout())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_when_stdin() {
        let pos = PathOrStdio::from("-".to_string());
        match pos {
            PathOrStdio::Stdio => {}
            _ => panic!("Expected Stdio variant"),
        }
    }

    #[test]
    fn test_when_path() {
        let path_str = "test.md".to_string();
        let pos = PathOrStdio::from(path_str.clone());
        match pos {
            PathOrStdio::Path(p) => {
                assert_eq!(p, PathBuf::from(path_str));
            }
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn test_with_temp_file_get_readable() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "Hello, world!").expect("Failed to write to temp file");

        let path_str = temp_file.path().to_str().unwrap().to_string();
        let pos = PathOrStdio::from(path_str);

        match pos {
            PathOrStdio::Path(_) => {
                let mut reader = pos.reader().expect("Failed to get reader");
                let mut content = String::new();
                reader
                    .read_to_string(&mut content)
                    .expect("Failed to read content");
                assert_eq!(content.trim(), "Hello, world!");
            }
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn test_with_temp_file_get_writable() {
        use std::io::Read;
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path_str = temp_file.path().to_str().unwrap().to_string();
        let pos = PathOrStdio::from(path_str.clone());

        match pos {
            PathOrStdio::Path(_) => {
                {
                    let mut writer = pos.writer().expect("Failed to get writer");
                    writeln!(writer, "Hello, writable world!").expect("Failed to write content");
                }

                let mut file = File::open(path_str).expect("Failed to open temp file");
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .expect("Failed to read content");
                assert_eq!(content.trim(), "Hello, writable world!");
            }
            _ => panic!("Expected Path variant"),
        }
    }
}
