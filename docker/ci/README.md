# CI - image builder

This folder contains files to make and push a docker image for building Qovery apps like the engine on Gitlab.

Once you've run "docker login" with qoveryrd credentials, it will push a new version to dockerhub.

Then you'll be able to add this image to gitlab CI (.gitlab-ci.yml)