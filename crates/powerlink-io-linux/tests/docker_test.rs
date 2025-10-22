use std::process::Command;

/// A guard struct to ensure `docker-compose down` is always called to clean up.
/// It runs when the struct goes out of scope at the end of the test function.
struct DockerComposeGuard {
    compose_file_path: String,
}

impl Drop for DockerComposeGuard {
    fn drop(&mut self) {
        println!("\n--- Tearing down Docker environment ---");
        // This command runs regardless of test success or failure, ensuring cleanup.
        // The `-v` flag removes volumes, ensuring a completely clean state.
        let _ = Command::new("docker")
            .args(["compose", "-f", &self.compose_file_path, "down", "-v"])
            .status(); // We don't panic here to ensure cleanup is best-effort.
    }
}

/// This test orchestrates the Docker environment. It builds and runs the
/// containers in the foreground, captures all their logs, and ensures
/// everything is torn down afterward.
#[test]
fn run_docker_integration_test() {
    // Path is relative to the crate's root where `cargo test` is executed.
    let compose_file_path = "tests/loopback_test_resources/docker-compose.yml";

    // Create the guard. When `_guard` goes out of scope at the end of this
    // function, its `drop` method will automatically run `docker-compose down`.
    let _guard = DockerComposeGuard {
        compose_file_path: compose_file_path.to_string(),
    };

    println!("--- Starting Docker Integration Test via 'docker compose' ---");

    // Execute `docker compose up`. This command will:
    // 1. Build the image if it's not already built.
    // 2. Start both the `cn` and `mn` containers.
    // 3. Stream the logs from BOTH containers to this command's stdout/stderr.
    // 4. Wait for the `mn` container to exit (due to `--exit-code-from`).
    // 5. Exit with the same code as the `mn` container.
    let output = Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_file_path,
            "up",
            "--build",
            "--exit-code-from", // Tells compose to return the exit code of one service
            "mn",
        ])
        .output() // Capture stdout and stderr
        .expect("Failed to execute docker compose up command.");

    // Print the captured, interleaved logs from both containers.
    println!(
        "--- docker-compose stdout ---\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    eprintln!(
        "--- docker-compose stderr ---\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Assert that the `docker-compose up` command itself was successful.
    // This will be true if the test running inside the `mn` container exited with code 0.
    assert!(
        output.status.success(),
        "The Docker integration test failed. Check the logs above."
    );
}

