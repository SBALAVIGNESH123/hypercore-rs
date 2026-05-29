use std::env;
use std::path::Path;
use std::process::Command;

pub fn get_fixture() -> Option<String> {
    let path = "tests/fixtures/tinystories.gguf";
    if Path::new(path).exists() {
        return Some(path.to_string());
    }

    if env::var("TEST_MODEL").unwrap_or_default() == "download"
        || env::var("STRESS_TEST").unwrap_or_default() == "1"
    {
        println!("Downloading tinystories.gguf...");
        let status = Command::new("curl.exe")
            .args([
                "-L", 
                "-o", path,
                "https://huggingface.co/raincandy-u/TinyStories-656K-Q8_0-GGUF/resolve/main/tinystories-656k-q8_0.gguf"
            ])
            .status()
            .expect("Failed to execute curl");

        if status.success() {
            return Some(path.to_string());
        } else {
            panic!("Failed to download model fixture.");
        }
    }

    None
}
