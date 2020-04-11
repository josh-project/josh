
josh-proxy
==========

Josh provides a http proxy server that can be used with any git hosting service that supports
the http transfer protocol.

It needs the url of the upstream server and a local directory to store it's data.
Optionally a port to listen on can be specified::

    $ josh-proxy --local=/tmp/josh --remote=https://github.com& --port=8000

Will run a proxy for github.com.

Url syntax
----------

Urls for filtered repositories are constructed by appending the filter specification to the
original path on the upstream host::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs.git

Will clone a repository with just the documentation of josh itself.

Note that this url needs to contain the `.git` suffix two times:
Once after the original path and once more after the filter spec.
