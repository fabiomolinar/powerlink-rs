// Test runner that executes the Docker Compose setup.
// This test does NOT run inside the container.

#![cfg(target_os = "linux")] // Docker setup relies on Linux commands

use std::process::{Command, Output};
use std::path::PathBuf;
use std::env;

// Guard struct to ensure docker-compose down is always called
struct DockerComposeGuard {
    compose_file_path: PathBuf,
}

impl DockerComposeGuard {
    fn new(compose_file_path: PathBuf) -> Self {
        Self { compose_file_path }
    }

    fn run_command(&self, args: &[&str]) -> std::io::Result<Output> {
        let mut cmd = Command::new("docker-compose");
        cmd.arg("-f")
           .arg(&self.compose_file_path)
           .args(args);

        println!("--- Running Docker Command: docker-compose -f {:?} {} ---", self.compose_file_path, args.join(" "));
        cmd.output()
    }
}

impl Drop for DockerComposeGuard {
    fn drop(&mut self) {
        println!("--- Tearing down Docker environment ---");
        match self.run_command(&["down", "--volumes", "--remove-orphans"]) {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!(
                        "--- docker-compose down failed ---\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                     println!("--- Docker environment teardown successful ---");
                }
            }
            Err(e) => {
                eprintln!("--- Failed to execute docker-compose down: {} ---", e);
            }
        }
    }
}


#[test]
// #[ignore] // Removed ignore: This should run by default with `cargo test`
fn run_docker_integration_test() {
    println!("--- Starting Docker Integration Test via 'docker compose' ---");

    // Construct the path relative to the crate root where this test lives
    let mut crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let compose_file_path = crate_root.join("tests/loopback_test_resources/docker-compose.yml");

    if !compose_file_path.exists() {
         panic!("docker-compose.yml not found at expected path: {:?}", compose_file_path);
    }

    let guard = DockerComposeGuard::new(compose_file_path);

    // Run docker-compose up.
    // --build: Ensures the image is built if it doesn't exist or Dockerfile changed.
    // --exit-code-from mn: Makes docker-compose exit with the code of the 'mn' service.
    // --remove-orphans: Cleans up any containers not defined in the compose file.
    let output = guard.run_command(&[
        "up",
        "--build",
        "--exit-code-from", "mn",
        "--remove-orphans", // Add this for cleaner runs
    ]).expect("Failed to execute docker-compose up command");

    // Print the combined output from docker-compose (which includes logs from both containers)
    println!("--- docker-compose stdout ---\n{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("--- docker-compose stderr ---\n{}", String::from_utf8_lossy(&output.stderr));

    // Check if docker-compose itself (and thus the 'mn' test container) exited successfully
    assert!(output.status.success(), "The Docker integration test failed. Check the logs above.");

    // The guard's Drop implementation will automatically run `docker-compose down` here
    println!("--- Docker Integration Test Completed ---");
}

