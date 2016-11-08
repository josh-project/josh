use shell::Shell;
use std::path::Path;

pub struct FileLock
{
    name: String
}

impl FileLock
{
    pub fn new(name: &str) -> FileLock
    {
        let shell = Shell { cwd: Path::new("/tmp/").to_path_buf() };

        loop {
            let (_, err) = shell.command(&format!("mkdir {}", name));
            if err == "" {
                break;
            }
        }

        FileLock { name: name.to_string() }
    }
}

impl Drop for FileLock
{
    fn drop(&mut self)
    {
        let shell = Shell { cwd: Path::new("/tmp/").to_path_buf() };
        shell.command(&format!("rm -Rf {}", &self.name));
    }
}

