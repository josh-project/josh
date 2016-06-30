[![Build Status](https://travis-ci.org/esrlabs/centralgithook.svg?branch=master)](http://travis-ci.org/esrlabs/centralgithook)
[![Coverage Status](https://coveralls.io/repos/github/esrlabs/centralgithook/badge.svg?branch=master)](https://coveralls.io/github/esrlabs/centralgithook?branch=master)

this tool exists to support the following usecases:

* compose one integration git-repository that uses several other git repos as modules
* allow commits to this integration repo and apply the relevant changes to the original git repos
* ...

can be used as a gerrit hook
