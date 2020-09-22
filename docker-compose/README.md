# Docker compose (use it to test in local)

## How to use it ?

* `./helper.sh generate_tmp_libs_tar` -> generate libs.tgz in `/tmp/qovery-libs`
* build docker image `./helper.sh build_image`
* change the image in `build.yaml` for engine:image to use image you just generated before
* run `docker-compose -f build.yaml up`