
Working with workspaces
=======================

For the sake of this example we will assume a ``josh-proxy`` instance is running and serving a
repo on ``http://josh/world.git`` with some shared code in ``shared``.

Create a new workspace
----------------------

To create a new workspace in the path ``ws/hello`` simply clone it as if it already exists:

    $ git clone http://josh/world.git:workspace=ws/hello.git

``git`` will report that you appear to have cloned an empty repository if that path does not
yet exist.
If you don't get this message it means that the path already exists in the repo but may
not yet have configured any path mappings.

The next step is to add some path mapping to the ``workspace.josh`` file in the root of the
workspace:

    $ cd hello
    $ echo "mod/a = :/shared/a" > workspace.josh

And and commit the changes:

    $ git add workspace.josh
    $ git commit -m "add workspace"

If the path did not exist previously, the resulting commit will be a root commit that does not share
any history with the ``world.git`` repo.
This means a normal ``git push`` will be rejected at this point.
To get correct history, the
resulting commit needs to be a based on the history that already exists in ``world.git``.
There is however no way to do this locally, because we don't have the data required for this.
Also, the resulting tree should contain the contents of ``shared/a`` mapped to ``mod/a`` which
means it needs to be produced on the server side because we don't have the files to put there.

To accomplish that push with the create option:

    $ git push -o create origin master


>**Note**: While it is perfectly possible to use Josh without a code review system,
>it is strongly recommended to use some form of code review to be able to inspect commits
>created by Josh before they get into the immutable history of your main repository.

As the resulting commit is created on the server side we need to get it from the server:

    $ git pull --rebase

Now you should see ``mod/a`` populated with the content of the shared code.


Map a shared path into a workspace
----------------------------------

To add shared path to a location in the workspace that does not exist yet, first add an
entry to the ``workspace.josh`` file and commit that.

At this point the path is of course empty to the commit needs to be pushed to the server.
When the same commit is then fetched back it will have the mapped path populated with the
shared content.

Publish a non-shared path into a shared location
------------------------------------------------

The steps here are exactly the same as for the mapping example above. The only difference being
that the path already exists in the workspace but not in the shared location.

Remove a mapping
----------------

To remove a mapping remove the corresponding entry from the ``workspace.josh`` file.
The content of the previously shared path will stay in the workspace. That means the main
repo will have two copies of that path from that point on. Effectivly creating a fork of that code.

Remove a mapped path
--------------------

To remove a mapped path as well as it's contents, remove the entry from the
``workspace.josh`` file and also remove the path inside the workspace using ``git rm``.


