use std::path::Path;
use std::process::Command;

/// A simple RAII guard to ensure `docker compose down` is called.
/// When an instance of this struct goes out of scope (at the end of the test function,
/// either by success or panic), its `drop` implementation will be called,
/// automatically cleaning up the Docker environment.
struct DockerComposeGuard<'a> {
    compose_command: &'a str,
    compose_file_path: &'a str,
}

impl<'a> Drop for DockerComposeGuard<'a> {
    fn drop(&mut self) {
        println!("\n--- Tearing down Docker environment ---");

        let mut command;
        let args;

        if self.compose_command == "docker-compose" {
            command = Command::new("docker-compose");
            args = vec!["-f", self.compose_file_path, "down", "-v"];
        } else {
            command = Command::new("docker");
            args = vec!["compose", "-f", self.compose_file_path, "down", "-v"];
        }

        let down_output = command
            .args(&args)
            .output()
            .expect("Failed to execute docker compose down command.");

        if !down_output.status.success() {
            eprintln!(
                "--- docker compose down stderr ---\n{}",
                String::from_utf8_lossy(&down_output.stderr)
            );
        }
    }
}

/// This test acts as a wrapper to run the Docker-based integration test.
/// It uses `std::process::Command` to execute `docker compose`.
#[test]
fn run_docker_integration_test() {
    // Ensure Docker is installed.
    if Command::new("docker").arg("--version").output().is_err() {
        panic!("Docker is not installed or not in PATH. Skipping Docker test.");
    }

    // Use `docker compose` which is the current standard, but fall back to `docker-compose`.
    let compose_command = if Command::new("docker")
        .arg("compose")
        .arg("--version")
        .output()
        .map_or(false, |out| out.status.success())
    {
        "compose"
    } else if Command::new("docker-compose")
        .arg("--version")
        .output()
        .map_or(false, |out| out.status.success())
    {
        "docker-compose"
    } else {
        panic!("Neither 'docker compose' nor 'docker-compose' found in PATH.");
    };

    println!(
        "--- Starting Docker Integration Test via 'docker {}' ---",
        compose_command
    );

    // Path is relative to the crate root, which is the CWD for `cargo test`.
    let compose_file_path = "tests/loopback_test_resources/docker-compose.yml";

    // Check that the compose file exists before trying to run it.
    assert!(
        Path::new(compose_file_path).exists(),
        "docker-compose.yml not found at expected path: {}",
        compose_file_path
    );

    // This guard will automatically call `docker compose down` when the test function finishes.
    let _guard = DockerComposeGuard {
        compose_command,
        compose_file_path,
    };

    // Build and run the docker-compose setup.
    let mut command;
    let args;

    if compose_command == "docker-compose" {
        command = Command::new("docker-compose");
        args = vec![
            "-f",
            compose_file_path,
            "up",
            "--build",
            "--abort-on-container-exit",
            "--exit-code-from",
            "mn",
        ];
    } else {
        command = Command::new("docker");
        args = vec![
            "compose",
            "-f",
            compose_file_path,
            "up",
            "--build",
            "--abort-on-container-exit",
            "--exit-code-from",
            "mn",
        ];
    }

    let output = command
        .args(&args)
        .output()
        .expect("Failed to execute docker compose up command.");

    // Print the stdout and stderr from the docker-compose command.
    println!(
        "--- docker compose stdout ---\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    eprintln!(
        "--- docker compose stderr ---\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Assert that the `mn` container (and thus the test) exited with a success code.
    assert!(
        output.status.success(),
        "The Docker integration test failed. Check the logs above."
    );
}

