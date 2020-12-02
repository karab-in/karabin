#!/bin/bash

new_tag="$1"

#sudo docker tag orgkarabin/karabin:$new_tag
#sudo docker push orgkarabin/karabin:$new_tag

docker tag prod_lemmy orgkarabin/karabin:v0.8.9-kbin
docker push orgkarabin/karabin:v0.8.9-kbin

git tag $new_tag
git push origin $new_tag
