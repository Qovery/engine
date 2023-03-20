FROM public.ecr.aws/r3m4q3r9/pub-mirror-debian:11.6 AS build

COPY hello.sh /hello.sh

FROM public.ecr.aws/r3m4q3r9/pub-mirror-debian:11.6
COPY --from=build /hello.sh /bin/hello.sh
ENTRYPOINT ["/bin/hello.sh"]
