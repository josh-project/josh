FROM golang:1.25

WORKDIR /go/src/github.com/git-lfs/lfs-test-server

COPY . .

RUN go build

EXPOSE 8080

CMD ["/go/src/github.com/git-lfs/lfs-test-server/lfs-test-server"]
