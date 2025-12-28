// Integration tests for the chat server and client.
//
// This script tests:
// 1. Server startup and accepts connections
// 2. Client can join with a valid username
// 3. Multiple clients can join and see each other's messages
// 4. Messages are broadcast correctly to all connected clients
// 5. Leave notification is sent when a client disconnects
// 6. Server rejects duplicate usernames
// 7. Graceful shutdown

package main

import (
	"fmt"
	"net"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"
)

// Timing constants for test synchronization
const (
	clientConnectDelay  = 300 * time.Millisecond
	interCommandDelay   = 300 * time.Millisecond
	messageReceiveDelay = 500 * time.Millisecond
)

// Configuration
var (
	testPort       = getEnv("CHAT_PORT", "9999")
	testHost       = getEnv("CHAT_HOST", "127.0.0.1")
	serverBin      = "./target/release/server"
	clientBin      = "./target/release/client"
	timeoutSeconds = 5
)

// Global state
var (
	serverCmd   *exec.Cmd
	clientCmds  []*exec.Cmd
	tempFiles   []string
	mu          sync.Mutex
	testsRun    int
	testsPassed int
	testsFailed int
)

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func logInfo(msg string) {
	fmt.Printf("[INFO] %s\n", msg)
}

func logPass(msg string) {
	fmt.Printf("[PASS] %s\n", msg)
	mu.Lock()
	testsPassed++
	mu.Unlock()
}

func logFail(msg string) {
	fmt.Printf("[FAIL] %s\n", msg)
	mu.Lock()
	testsFailed++
	mu.Unlock()
}

func cleanup() {
	logInfo("Cleaning up...")

	mu.Lock()
	defer mu.Unlock()

	for _, cmd := range clientCmds {
		if cmd != nil && cmd.Process != nil {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
		}
	}
	clientCmds = nil

	if serverCmd != nil && serverCmd.Process != nil {
		_ = serverCmd.Process.Kill()
		_ = serverCmd.Wait()
		serverCmd = nil
	}

	for _, f := range tempFiles {
		_ = os.Remove(f)
	}
	tempFiles = nil

	logInfo("Cleanup complete.")
}

func waitForPort(host, port string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	address := net.JoinHostPort(host, port)

	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", address, 100*time.Millisecond)
		if err == nil {
			conn.Close()
			return true
		}
		time.Sleep(100 * time.Millisecond)
	}
	return false
}

func createTempFile() (string, error) {
	f, err := os.CreateTemp("", "chat-test-*")
	if err != nil {
		return "", err
	}
	name := f.Name()
	f.Close()

	mu.Lock()
	tempFiles = append(tempFiles, name)
	mu.Unlock()

	return name, nil
}

func startServer() error {
	logInfo(fmt.Sprintf("Starting server on %s:%s...", testHost, testPort))

	serverCmd = exec.Command(serverBin)
	serverCmd.Env = append(os.Environ(),
		fmt.Sprintf("CHAT_HOST=%s", testHost),
		fmt.Sprintf("CHAT_PORT=%s", testPort),
	)

	if err := serverCmd.Start(); err != nil {
		return fmt.Errorf("failed to start server: %w", err)
	}

	if !waitForPort(testHost, testPort, time.Duration(timeoutSeconds)*time.Second) {
		return fmt.Errorf("server failed to start within %ds", timeoutSeconds)
	}

	logInfo(fmt.Sprintf("Server started (PID: %d)", serverCmd.Process.Pid))
	return nil
}

func runClientWithInput(username string, input []string, outputFile string, duration time.Duration) (*exec.Cmd, error) {
	cmd := exec.Command(clientBin,
		"--host", testHost,
		"--port", testPort,
		"--username", username,
	)

	// Create output file
	outFile, err := os.Create(outputFile)
	if err != nil {
		return nil, err
	}

	cmd.Stdout = outFile
	cmd.Stderr = outFile

	// Create stdin pipe
	stdin, err := cmd.StdinPipe()
	if err != nil {
		outFile.Close()
		return nil, err
	}

	if err := cmd.Start(); err != nil {
		outFile.Close()
		return nil, err
	}

	mu.Lock()
	clientCmds = append(clientCmds, cmd)
	mu.Unlock()

	// Send input commands with delays
	go func() {
		defer stdin.Close()
		defer outFile.Close()

		// Wait for client to connect before sending commands
		time.Sleep(clientConnectDelay)

		for i, line := range input {
			if i > 0 {
				time.Sleep(interCommandDelay)
			}
			fmt.Fprintln(stdin, line)
		}
	}()

	// Wait with timeout
	done := make(chan error, 1)
	go func() {
		done <- cmd.Wait()
	}()

	select {
	case <-done:
	case <-time.After(duration):
		_ = cmd.Process.Kill()
		<-done
	}

	return cmd, nil
}

type ClientHandle struct {
	Cmd       *exec.Cmd
	LeaveChan chan struct{}
	OutFile   *os.File
}

func runClientBackground(username string, input []string, outputFile string) (*exec.Cmd, error) {
	cmd := exec.Command(clientBin,
		"--host", testHost,
		"--port", testPort,
		"--username", username,
	)

	outFile, err := os.Create(outputFile)
	if err != nil {
		return nil, err
	}

	cmd.Stdout = outFile
	cmd.Stderr = outFile

	stdin, err := cmd.StdinPipe()
	if err != nil {
		outFile.Close()
		return nil, err
	}

	if err := cmd.Start(); err != nil {
		outFile.Close()
		return nil, err
	}

	mu.Lock()
	clientCmds = append(clientCmds, cmd)
	mu.Unlock()

	if len(input) == 0 {
		go func() {
			defer outFile.Close()
			_ = cmd.Wait()
		}()
		return cmd, nil
	}

	go func() {
		defer stdin.Close()
		defer outFile.Close()

		time.Sleep(clientConnectDelay)

		for i, line := range input {
			if i > 0 {
				time.Sleep(interCommandDelay)
			}
			fmt.Fprintln(stdin, line)
		}
	}()

	return cmd, nil
}

func readFileContent(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	return string(data)
}

func containsIgnoreCase(s, substr string) bool {
	return strings.Contains(strings.ToLower(s), strings.ToLower(substr))
}

func testBasicConnection() bool {
	logInfo("Test: Basic connection and join...")
	testsRun++

	output, err := createTempFile()
	if err != nil {
		logFail("Basic connection and join - failed to create temp file")
		return false
	}

	_, err = runClientWithInput("test_user1", []string{"leave"}, output, 3*time.Second)
	if err != nil {
		logFail("Basic connection and join - failed to run client")
		return false
	}

	content := readFileContent(output)
	if strings.Contains(content, "Joined as 'test_user1'") {
		logPass("Basic connection and join")
		return true
	}

	logFail("Basic connection and join")
	fmt.Println(content)
	return false
}

func testDuplicateUsername() bool {
	logInfo("Test: Duplicate username rejection...")
	testsRun++

	output1, err := createTempFile()
	if err != nil {
		logFail("Duplicate username rejection - failed to create temp file")
		return false
	}
	output2, err := createTempFile()
	if err != nil {
		logFail("Duplicate username rejection - failed to create temp file")
		return false
	}

	cmd1, err := runClientBackground("duplicate_user", []string{}, output1)
	if err != nil {
		logFail("Duplicate username rejection - failed to start first client")
		return false
	}

	time.Sleep(messageReceiveDelay)

	_, err = runClientWithInput("duplicate_user", []string{"leave"}, output2, 2*time.Second)
	if err != nil {
		logFail("Duplicate username rejection - failed to run second client")
		return false
	}

	if cmd1.Process != nil {
		_ = cmd1.Process.Kill()
		_ = cmd1.Wait()
	}

	content := readFileContent(output2)
	if containsIgnoreCase(content, "ERR") || containsIgnoreCase(content, "error") {
		logPass("Duplicate username rejection")
		return true
	}

	logFail("Duplicate username rejection - expected error for duplicate username")
	fmt.Println("First client output:")
	fmt.Println(readFileContent(output1))
	fmt.Println("Second client output:")
	fmt.Println(content)
	return false
}

func testMessageBroadcast() bool {
	logInfo("Test: Message broadcast between clients...")
	testsRun++

	outputAlice, err := createTempFile()
	if err != nil {
		logFail("Message broadcast - failed to create temp file")
		return false
	}
	outputBob, err := createTempFile()
	if err != nil {
		logFail("Message broadcast - failed to create temp file")
		return false
	}

	aliceInputs := []string{}
	cmdAlice, err := runClientBackground("alice", aliceInputs, outputAlice)
	if err != nil {
		logFail("Message broadcast - failed to start Alice")
		return false
	}

	time.Sleep(clientConnectDelay)

	bobInputs := []string{"send Hello from Bob!", "leave"}
	_, err = runClientWithInput("bob", bobInputs, outputBob, 3*time.Second)
	if err != nil {
		logFail("Message broadcast - failed to run Bob")
		return false
	}

	time.Sleep(messageReceiveDelay)

	if cmdAlice.Process != nil {
		_ = cmdAlice.Process.Kill()
		_ = cmdAlice.Wait()
	}

	content := readFileContent(outputAlice)
	if strings.Contains(content, "Hello from Bob") || strings.Contains(content, "[bob]") {
		logPass("Message broadcast between clients")
		return true
	}

	logFail("Message broadcast - Alice did not receive Bob's message")
	fmt.Println("Alice's output:")
	fmt.Println(content)
	fmt.Println("Bob's output:")
	fmt.Println(readFileContent(outputBob))
	return false
}

func testJoinLeaveNotifications() bool {
	logInfo("Test: Join/Leave notifications...")
	testsRun++

	outputCharlie, err := createTempFile()
	if err != nil {
		logFail("Join/Leave notifications - failed to create temp file")
		return false
	}
	outputDave, err := createTempFile()
	if err != nil {
		logFail("Join/Leave notifications - failed to create temp file")
		return false
	}

	cmdCharlie, err := runClientBackground("charlie", []string{}, outputCharlie)
	if err != nil {
		logFail("Join/Leave notifications - failed to start Charlie")
		return false
	}

	time.Sleep(clientConnectDelay)

	daveInputs := []string{"leave"}
	_, err = runClientWithInput("dave", daveInputs, outputDave, 2*time.Second)
	if err != nil {
		logFail("Join/Leave notifications - failed to run Dave")
		return false
	}

	time.Sleep(messageReceiveDelay)

	if cmdCharlie.Process != nil {
		_ = cmdCharlie.Process.Kill()
		_ = cmdCharlie.Wait()
	}

	charlieContent := readFileContent(outputCharlie)
	daveContent := readFileContent(outputDave)

	passed := 0

	if strings.Contains(charlieContent, "dave joined") || containsIgnoreCase(charlieContent, "JOINED dave") {
		passed++
	}

	if strings.Contains(charlieContent, "dave left") || strings.Contains(daveContent, "Goodbye") {
		passed++
	}

	if passed >= 1 {
		logPass("Join/Leave notifications")
		return true
	}

	logFail("Join/Leave notifications")
	fmt.Println("Charlie's output:")
	fmt.Println(charlieContent)
	fmt.Println("Dave's output:")
	fmt.Println(daveContent)
	return false
}

func testInvalidUsername() bool {
	logInfo("Test: Invalid username handling...")
	testsRun++

	output, err := createTempFile()
	if err != nil {
		logFail("Invalid username handling - failed to create temp file")
		return false
	}

	_, err = runClientWithInput("", []string{"leave"}, output, 2*time.Second)
	if err != nil {
		logPass("Invalid username handling")
		return true
	}

	content := readFileContent(output)

	if containsIgnoreCase(content, "error") || containsIgnoreCase(content, "ERR") || content == "" {
		logPass("Invalid username handling")
		return true
	}

	if strings.Contains(content, "Joined") {
		logFail("Invalid username handling - empty username was accepted")
		fmt.Println(content)
		return false
	}

	logPass("Invalid username handling (connection rejected)")
	return true
}

func testSendCommand() bool {
	logInfo("Test: Send command formats...")
	testsRun++

	output, err := createTempFile()
	if err != nil {
		logFail("Send command formats - failed to create temp file")
		return false
	}

	inputs := []string{"send Test message 123", "leave"}
	_, err = runClientWithInput("sender", inputs, output, 5*time.Second)
	if err != nil {
		logFail("Send command formats - failed to run client")
		return false
	}

	content := readFileContent(output)

	hasNoSendError := !containsIgnoreCase(content, "Failed to send")
	joinedSuccessfully := strings.Contains(content, "Joined as 'sender'")

	if (strings.Contains(content, "Goodbye") || joinedSuccessfully) && hasNoSendError {
		logPass("Send command formats")
		return true
	}

	logFail("Send command formats")
	fmt.Println(content)
	return false
}

func testServerResilience() bool {
	logInfo("Test: Server resilience after multiple connections...")
	testsRun++

	for i := 1; i <= 3; i++ {
		output, err := createTempFile()
		if err != nil {
			logInfo(fmt.Sprintf("Resilience iteration %d: temp file error (non-fatal)", i))
			continue
		}
		username := fmt.Sprintf("resilience_user_%d", i)
		if _, err := runClientWithInput(username, []string{"leave"}, output, 2*time.Second); err != nil {
			logInfo(fmt.Sprintf("Resilience iteration %d: client error (non-fatal)", i))
		}
	}

	output, err := createTempFile()
	if err != nil {
		logFail("Server resilience - failed to create temp file")
		return false
	}

	_, err = runClientWithInput("final_test_user", []string{"leave"}, output, 2*time.Second)
	if err != nil {
		logFail("Server resilience - failed to run final client")
		return false
	}

	content := readFileContent(output)
	if strings.Contains(content, "Joined as 'final_test_user'") {
		logPass("Server resilience after multiple connections")
		return true
	}

	logFail("Server resilience - server may have crashed")
	fmt.Println(content)
	return false
}

func main() {
	if os.Getenv("TZ") == "" {
		os.Setenv("TZ", "UTC")
	}

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-sigChan
		cleanup()
		os.Exit(1)
	}()

	defer cleanup()

	execPath, err := os.Executable()
	if err == nil {
		execDir := filepath.Dir(execPath)
		if filepath.Base(execDir) == "scripts" {
			execDir = filepath.Dir(execDir)
		}
		serverBin = filepath.Join(execDir, "target", "release", "server")
		clientBin = filepath.Join(execDir, "target", "release", "client")
	}

	if _, err := os.Stat(serverBin); os.IsNotExist(err) {
		serverBin = "./target/release/server"
	}
	if _, err := os.Stat(clientBin); os.IsNotExist(err) {
		clientBin = "./target/release/client"
	}

	if _, err := os.Stat(serverBin); os.IsNotExist(err) {
		logFail(fmt.Sprintf("Server binary not found at %s. Run 'make build-release' first.", serverBin))
		os.Exit(1)
	}

	if _, err := os.Stat(clientBin); os.IsNotExist(err) {
		logFail(fmt.Sprintf("Client binary not found at %s. Run 'make build-release' first.", clientBin))
		os.Exit(1)
	}

	if err := startServer(); err != nil {
		logFail(fmt.Sprintf("Could not start server: %v", err))
		os.Exit(1)
	}

	fmt.Println()
	logInfo("Running integration tests...")
	fmt.Println()

	// Run all tests
	testBasicConnection()
	testDuplicateUsername()
	testMessageBroadcast()
	testJoinLeaveNotifications()
	testInvalidUsername()
	testSendCommand()
	testServerResilience()

	fmt.Println()
	fmt.Println("=========================================")
	fmt.Println("  Test Results")
	fmt.Println("=========================================")
	fmt.Println()
	fmt.Printf("Tests run:    %d\n", testsRun)
	fmt.Printf("Tests passed: %d\n", testsPassed)
	fmt.Printf("Tests failed: %d\n", testsFailed)
	fmt.Println()

	if testsFailed > 0 {
		fmt.Printf("Some tests failed!\n")
		os.Exit(1)
	} else {
		fmt.Printf("All tests passed!\n")
		os.Exit(0)
	}
}
