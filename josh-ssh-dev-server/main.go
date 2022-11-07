package main

import (
	"flag"
	"fmt"
	"github.com/gliderlabs/ssh"
	"io"
	"log"
	"os"
	"os/exec"
	"sync"
)

const DefaultServerPort = 23186

var CurrentTask = 1
var TaskMutex sync.Mutex

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
			_, err := io.Copy(os.Stderr, stderr)
			if err != nil {
				return
			}
		}()

		_, err = io.Copy(session, stdout)
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
