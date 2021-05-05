
josh-filter
===========

Command to rewrite history using ``josh`` filter specs.
By default it will use ``HEAD`` as input and update ``FILTERED_HEAD`` with the filtered
history, taking a filter specification as argument.

git-sync
========

A utility to make working with server side rewritten commits easier.
Those commits frequently get produced when making changes to ``workspace.josh`` files.

The command should be put into ``PATH`` and can be used as a drop-in replacement for ``git push``.
It enables the server to *return* commits back to the client after a push. This is done by parsing
the messages sent back by the server for announcements of rewritten commits and then fetching
those to update the local references.
In case of a normal git server that does not rewrite anything, ``git sync`` will do exactly the
same as ``git push``, also accepting the same arguments.
