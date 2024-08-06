  $ git init -q 1> /dev/null

Initial commit
  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Apply prefix filter
  $ josh-filter -s :prefix=subtree refs/heads/master --update refs/heads/filtered
  [1] :prefix=subtree

  $ git log --graph --pretty=%s refs/heads/filtered
  * add file1

  $ git show refs/heads/filtered
  commit f5ea0c2feb26f846b28627cf6275682eba3f3f3a
  Author: Josh <josh@example.com>
  Date:   Thu Apr 7 22:13:13 2005 +0000
  
      add file1
  
  diff --git a/subtree/file1 b/subtree/file1
  new file mode 100644
  index 0000000..a024003
  --- /dev/null
  +++ b/subtree/file1
  @@ -0,0 +1 @@
  +contents1

  $ git ls-tree --name-only -r refs/heads/filtered
  subtree/file1

  $ josh-filter ":join(f5ea0c2feb26f846b28627cf6275682eba3f3f3a:prefix=subtree)"