[![Build Status](https://travis-ci.org/marcmo/centralgithook.svg?branch=master)](http://travis-ci.org/marcmo/centralgithook) [![Appveyor Build status](https://ci.appveyor.com/api/projects/status/vv4t6mfr25p61a6p?svg=true)](https://ci.appveyor.com/project/marcmo/centralgithook)

this tool exists to support the following usecases:

* compose one integration git-repository that uses several other git repos as modules
* allow commits to this integration repo and apply the relevant changes to the original git repos
* ...

can be used as a gerrit hook
