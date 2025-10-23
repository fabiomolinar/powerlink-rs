// Test runner that executes the Docker Compose setup.
// This test does NOT run inside the container.

#![cfg(target_os = "linux")] // Docker setup relies on Linux commands

use std::process::{Command, Output};
use std::path::PathBuf;
use std::env;

/// Guard struct to ensure docker-compose down is always called
struct DockerComposeGuard {
    compose_file_path: PathBuf,
}

impl DockerComposeGuard {
    fn new(compose_file_path: PathBuf) -> Self {
        Self { compose_file_path }
    }

    /// Runs a docker-compose command (e.g., "down", "up")
    fn run_compose_command(&self, args: &[&str]) -> std::io::Result<Output> {
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
        match self.run_compose_command(&["down", "--volumes", "--remove-orphans"]) {
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

/// Helper function to find the docker-compose file.
fn get_compose_file_path() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let compose_file_path = crate_root.join("tests/loopback_test_resources/docker-compose.yml");
    if !compose_file_path.exists() {
         panic!("docker-compose.yml not found at expected path: {:?}", compose_file_path);
    }
    compose_file_path
}

/// Helper function to run a specific integration test case via Docker.
fn run_docker_test(test_name: &str) {
    let compose_file_path = get_compose_file_path();
    
    // Set the environment variable *for the docker-compose command*
    unsafe { env::set_var("POWERLINK_TEST_TO_RUN", test_name) };

    // The guard ensures `docker-compose down` is called even if the test panics
    let guard = DockerComposeGuard::new(compose_file_path);

    // Run docker-compose up.
    // --build: Ensures the image is built if it doesn't exist or Dockerfile changed.
    // --abort-on-container-exit: Stops all containers if any container stops.
    let output = guard.run_compose_command(&[
        "up",
        "--build",
        "--abort-on-container-exit",
    ]).expect("Failed to execute docker-compose up command");

    // Print the combined output from docker-compose (which includes logs from both containers)
    println!("--- docker-compose stdout ---\n{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("--- docker-compose stderr ---\n{}", String::from_utf8_lossy(&output.stderr));

    // Check if docker-compose itself exited successfully.
    // If a container fails (test fails), `up --abort-on-container-exit` will cause
    // docker-compose to return a non-zero exit code.
    assert!(output.status.success(), "The Docker integration test '{}' failed. Check the logs above.", test_name);
}


#[test]
fn test_ident_request_sequence() {
    println!("--- [HOST] Running Test: test_cn_responds_to_ident_request ---");
    run_docker_test("test_cn_responds_to_ident_request");
}

#[test]
fn test_sdo_read_sequence() {
    println!("--- [HOST] Running Test: test_sdo_read_by_index_over_asnd ---");
    run_docker_test("test_sdo_read_by_index_over_asnd");
}
