
FROM golang:1.16 AS build

COPY hello.go /go/src/project/hello.go
WORKDIR /go/src/project
RUN go build hello.go

FROM scratch
COPY --from=build /go/src/project/hello /bin/hello
ENTRYPOINT ["/bin/hello"]
