
History filtering
=================

`josh` operates by transforming commits by applying one or more `filters` to them.
As any commit in `git` represents not just a single point in time but also it's whole
history, applying a `filter` to a commit produces an entire new history.
The result of a `filter` is a normal git commit and therefore can be filtered again,
making filters chainable.

Syntax
------

Filters are specified as::

    :filter=parameter

And chained::

    :filter1=parameter1:filter2=parameter2

The only exception is the subdirectory filter ``:/`` it does not have a
spelled out name and also no ``=`` in front of it's parameter. It can
also be chained without the ``:``. Therefore ``:/a/b/c`` is exactly
the same as ``:/a:/b:/c``.

Available filters:

``:/a``
    Only take the selected subdirectory from the commits tree and
    make it the root of the filtered commit

``:prefix=a``
    Take the whole original tree and move it into the subdirectory ``a``

``:workspace=a``
    The same as ``:/a`` but also looks for a workspace file and add extra
    paths to the filtered tree.
    (see :doc:`workspace`)
