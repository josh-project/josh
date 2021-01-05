
History filtering
=================

Josh transforms commits by applying one or more filters to them. As any
commit in git represents not just a single point in time but also its entire
history, applying a filter to a commit produces an entirely new history.
The result of a filter is a normal git commit and therefore can be filtered again,
making filters chainable.

Syntax
------

Filters are generally specified as::

    :filter=parameter

And chained via colons::

    :filter1=parameter1:filter2=parameter2

The only exception is the subdirectory filter ``:/``. It does not have a written name
in the syntax and also no ``=`` in front of its parameter. It can also be chained
without the ``:``. Therefore ``:/a/b/c`` is exactly the same as ``:/a:/b:/c``.

Available filters
-----------------

``:/a``
    Take only the selected subdirectory from the commits tree and make it the root
    of the filtered commit

``:prefix=a``
    Take the entire original tree and move it into subdirectory ``a``

``:workspace=a``
    The same as ``:/a`` but also looks for a workspace file and adds extra
    paths to the filtered tree.
    (see :doc:`workspace`)

Repository naming
-----------------

By default, a git URL is used to point to the remote repository to download `and also` to dictate
how the local repository shall be named.  It's important to learn that the last name in the URL is
what the local git client will name the new, local repository. For example::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs.git

will create the new repository at directory ``docs``, as ``docs.git`` is the last name in the URL.

By default, this leads to rather odd-looking repositories when the ``prefix`` filter is the final
filter of a URL::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs:prefix=josh-docs.git

This will still clone just the josh documentation, but the final directory structure will look like
this::

    - prefix=josh-docs
      - josh-docs
        - <docs>

Having the root repository directory name be the fully-specified filter is most likely not what was
intended. This results from git's reuse and repurposing of the remote URL, as ``prefix=josh-docs``
is the final name in the URL. With no other alternatives, this gets used for the repository name.

To explicitly specify a repository name, provide the desired name after the URL when cloning a new
repository::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs:prefix=josh-docs.git my-repo

Filter order matters
--------------------

Filters are applied in the left-to-right order they are given in the URL, and they are `not`
commutative.

For example, this (familiar) command will check out just the josh documentation, and store it in a
subdirectory named ``josh-docs``::

    $ git clone http://localhost:8000/esrlabs/josh.git:/docs:prefix=josh-docs.git

However, `this` command will exit with the error that an empty reply was received from the server::

    $ git clone http://localhost:8000/esrlabs/josh.git:prefix=josh-docs:/docs.git

What's happening in the latter command is that because the prefix filter is applied first, the
entire ``josh`` repository already lives within the ``josh-docs`` directory, as it was just
transformed to exist there. Thus, to still get the docs, the command would need to be::

    $ git clone http://localhost:8000/esrlabs/josh.git:prefix=josh-docs:/josh-docs/docs.git

which will contain the josh documentation at the base of the repository. We've lost the prefix, what
gives?? Because the original git tree was already transformed, and then the subdirectory filter
was applied to pull documentation from ``josh-docs/docs``, the prefix is gone - it was filtered out
again by the subdirectory filter. Thus, the order in which filters are provided is crucial, as each
filter further transforms the latest transformation of the tree.
