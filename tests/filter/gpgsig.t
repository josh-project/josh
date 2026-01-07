  $ git init -q 1> /dev/null

Initial commit
  $ echo contents1 > file1
  $ git add .
  $ git commit -m "add file1" 1> /dev/null

Now create another commit for the same tree, but with a gpgsig
  $ git rev-parse HEAD
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  $ git cat-file commit HEAD
  tree 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72
  author Josh <josh@example.com> 1112911993 +0000
  committer Josh <josh@example.com> 1112911993 +0000
  
  add file1
  $ git hash-object -t commit -w --stdin <<EOF
  > tree 3d77ff51363c9825cc2a221fc0ba5a883a1a2c72
  > author Josh <josh@example.com> 1112911993 +0000
  > committer Josh <josh@example.com> 1112911993 +0000
  > gpgsig hello
  > 
  > add file1
  > EOF
  cb22ebb8e47b109f7add68b1043e561e0db09802
  $ git reset --hard cb22ebb8e47b109f7add68b1043e561e0db09802 1>/dev/null

Apply a josh round-trip to this.
  $ josh-filter :prefix=extra refs/heads/master --update refs/heads/filtered
  778a3a29060a379a963e1cd38891d30d5cf321e4
  $ josh-filter :/extra refs/heads/filtered --update refs/heads/double-filtered
  cb22ebb8e47b109f7add68b1043e561e0db09802

And compare. Should be the same commit for both.
If 0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb shows up then the signature was lost.
  $ git diff master double-filtered
  $ git rev-parse master double-filtered
  cb22ebb8e47b109f7add68b1043e561e0db09802
  cb22ebb8e47b109f7add68b1043e561e0db09802

Remove the signature, the shas are different.
  $ josh-filter :unsign refs/heads/master --update refs/heads/filtered -s
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  [1] :unsign
  [1] sequence_number
  $ git rev-parse master filtered
  cb22ebb8e47b109f7add68b1043e561e0db09802
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  $ josh-filter --reverse :unsign refs/heads/double-filtered --update refs/heads/filtered -s
  cb22ebb8e47b109f7add68b1043e561e0db09802
  [1] :unsign
  [1] sequence_number
  $ git rev-parse master double-filtered
  cb22ebb8e47b109f7add68b1043e561e0db09802
  cb22ebb8e47b109f7add68b1043e561e0db09802

Round trip does not work but reversed works since the commit exists
  $ josh-filter :prefix=extra:unsign refs/heads/master --update refs/heads/filtered
  6bf53d368ae730dbe6210e5671f08e6998b83cb4
  $ josh-filter :/extra refs/heads/filtered --update refs/heads/double-filtered
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  $ git branch reversed
  $ josh-filter --reverse :prefix=extra:unsign refs/heads/reversed --update refs/heads/filtered
  cb22ebb8e47b109f7add68b1043e561e0db09802
  $ git rev-parse master double-filtered reversed
  cb22ebb8e47b109f7add68b1043e561e0db09802
  0b4cf6c9efbbda1eada39fa9c1d21d2525b027bb
  cb22ebb8e47b109f7add68b1043e561e0db09802
