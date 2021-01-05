
josh-proxy
==========

Josh provides an HTTP proxy server that can be used with any git hosting service which communicates
via HTTP.

It needs the URL of the upstream server and a local directory to store its data.
Optionally, a port to listen on can be specified. For example, running a local ``josh-proxy``
instance for github.com on port 8000::

    $ josh-proxy --local=/tmp/josh --remote=https://github.com --port=8000

For a first example of how to make use of josh, just the josh documentation can be checked out as
its own repository via this command::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs.git

.. note::

    This URL needs to contain the `.git` suffix twice: once after the original path and once more
    after the filter spec.

URL syntax and breakdown
------------------------

This is the URL of a ``josh-proxy`` instance::

    http://localhost:8000

This is the repository location on the upstream host on which to perform the filter operations::

    /esrlabs/josh.git

This is the set of filter operations to perform::

    :/docs.git

Much more information on the available filters and the syntax of all filters is covered in detail in
the :doc:`filters` section.
