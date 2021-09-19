
josh-proxy
==========

Josh provides an HTTP proxy server that can be used with any git hosting service which communicates
via HTTP.

It needs the URL of the upstream server and a local directory to store its data.
Optionally, a port to listen on can be specified. For example, running a local ``josh-proxy``
instance for github.com on port 8000:

    $ docker run -p 8000:8000 -e JOSH_REMOTE=https://github.com -v josh-vol:/data/git joshproject/josh-proxy:latest

>**Note**: The proxy is semantically stateless. The data inside the docker volume is only persisted across
>runs for performance reasons. This has two important implications for deployment:
> 1) The data does not need to be backed up unless working with very large repos where rebuilding would
> be very expensive. And 2) Multiple instances of josh-proxy can be used interchangeably for availability
> or load balancing purposes.

For a first example of how to make use of josh, just the josh documentation can be checked out as
its own repository via this command:

    $ git clone http://localhost:8000/josh-project/josh.git:/docs.git

>**Note**: This URL needs to contain the `.git` suffix twice: once after the original path
> and once more after the filter spec.

`josh-proxy` supports read and write access to the repository, so when making changes
to any files in the filtered repository, you can just commit and push them
like you are used to.

URL syntax and breakdown
------------------------

This is the URL of a ``josh-proxy`` instance:

    http://localhost:8000

This is the repository location on the upstream host on which to perform the filter operations:

    /josh-project/josh.git

This is the set of filter operations to perform:

    :/docs.git

Much more information on the available filters and the syntax of all filters is covered in detail in
the [filters](./filters.md) section.

Repository naming
-----------------

By default, a git URL is used to point to the remote repository to download _and also_ to dictate
how the local repository shall be named.  It's important to learn that the last name in the URL is
what the local git client will name the new, local repository. For example:

    $ git clone http://localhost:8000/josh-project/josh.git:/docs.git

will create the new repository at directory ``docs``, as ``docs.git`` is the last name in the URL.

By default, this leads to rather odd-looking repositories when the ``prefix`` filter is the final
filter of a URL:

    $ git clone http://localhost:8000/josh-project/josh.git:/docs:prefix=josh-docs.git

This will still clone just the josh documentation, but the final directory structure will look like
this:

    - prefix=josh-docs
      - josh-docs
        - <docs>

Having the root repository directory name be the fully-specified filter is most likely not what was
intended. This results from git's reuse and repurposing of the remote URL, as ``prefix=josh-docs``
is the final name in the URL. With no other alternatives, this gets used for the repository name.

To explicitly specify a repository name, provide the desired name after the URL when cloning a new
repository:

    $ git clone http://localhost:8000/josh-project/josh.git:/docs:prefix=josh-docs.git my-repo
