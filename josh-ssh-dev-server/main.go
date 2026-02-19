package main

import (
	"flag"
	"fmt"
	"github.com/gliderlabs/ssh"
	"io"
	"log"
	"net"
	"os"
	"os/exec"
	"path"
	"sync"
)

var CurrentTask = 1
var TaskMutex sync.Mutex

const (
	DefaultServerPort = 23186
)

// CreateAgentListener
//
// SSH client library does provide this function,
// but we can't use it because the string constants
// it uses make the socket path exceed the limit
// of 108 bytes:
//
// https://man7.org/linux/man-pages/man7/unix.7.html
//
// We use shorter constants here instead
func CreateAgentListener() (net.Listener, error) {
	const (
		AgentTempDir    = "agent"
		AgentListenFile = "agent"
	)

	dir, err := os.MkdirTemp("", AgentTempDir)
	if err != nil {
		return nil, err
	}

	socketPath := path.Join(dir, AgentListenFile)
	l, err := net.Listen("unix", socketPath)

	if err != nil {
		return nil, err
	}
	return l, nil
}

func runServer(port uint, shell string) {
	ssh.Handle(func(session ssh.Session) {
		if len(session.Command()) == 0 {
			_, _ = io.WriteString(session, "Interactive invocation is not supported\n")
			_ = session.Exit(1)
			return
		}

		cmd := exec.Command(shell, "-c", session.RawCommand())

		stdin, _ := cmd.StdinPipe()
		stdout, _ := cmd.StdoutPipe()
		stderr, _ := cmd.StderrPipe()

		if ssh.AgentRequested(session) {
			listener, err := CreateAgentListener()
			if err != nil {
				log.Printf("Failed to create agent listener")
				log.Printf(err.Error())
				return
			}

			defer func(listener net.Listener) {
				err := listener.Close()
				if err != nil {
					log.Printf("Failed to close agent listener")
				}
			}(listener)

			log.Printf("Starting agent listener at %s\n", listener.Addr())

			go ssh.ForwardAgentConnections(listener, session)
			cmd.Env = append(session.Environ(), fmt.Sprintf("%s=%s",
				"SSH_AUTH_SOCK", listener.Addr().String()))
		}

		err := cmd.Start()
		if err != nil {
			_, _ = io.WriteString(session.Stderr(), err.Error()+"\n")
			return
		}

		TaskMutex.Lock()
		CurrentTask = CurrentTask + 1
		taskId := CurrentTask
		TaskMutex.Unlock()

		log.Printf("started subprocess with task_id %d\n", taskId)

		go func() {
			_, err := io.Copy(stdin, session)
			if err != nil {
				return
			}
		}()

		go func() {
			_, err := io.Copy(session.Stderr(), stderr)
			if err != nil {
				return
			}
		}()

		_, err = io.Copy(session, stdout)
		if err != nil {
			return
		}

		_ = cmd.Wait()
		log.Printf("subprocess with task_id %d exited\n", taskId)

		_ = session.Exit(0)
	})

	log.Printf("starting ssh server on port %d...\n", port)
	log.Fatal(ssh.ListenAndServe(fmt.Sprintf(":%d", port), nil))
}

func main() {
	shellPath, err := exec.LookPath("sh")
	if err != nil {
		log.Println("Could not find default shell (sh) executable")
		os.Exit(1)
	}

	port := flag.Uint("port", DefaultServerPort, "Port to listen on")
	shell := flag.String("shell", shellPath, "Shell to use for commands")
	flag.Parse()
	runServer(*port, *shell)
}
