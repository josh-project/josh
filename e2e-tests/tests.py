import unittest
import commands
from commands import getoutput
from commands import getstatusoutput

import os
dir_path = os.path.dirname(os.path.realpath(__file__))

def getoutput(x):
    if x.startswith("ssh"):
        x = "ssh -i keys/user1/id_rsa" + x[3:]
    # elif x.startswith("git"):
    else:
        x = 'GIT_SSH_COMMAND="ssh -i keys/user1/id_rsa" ' + x

    print "call:", x

    o = commands.getoutput(x)
    print "output:", o
    return o

def in_data0(subdir, cmd):
    cmd = 'GIT_SSH_COMMAND="ssh -i {}/keys/user1/id_rsa" {}'.format(dir_path, cmd)
    return getoutput("cd _data0/{} && {}".format(subdir,cmd))

def in_data1(subdir, cmd):
    cmd = 'GIT_SSH_COMMAND="ssh -i {}/keys/user1/id_rsa" {}'.format(dir_path, cmd)
    return getoutput("cd _data1/{} && {}".format(subdir,cmd))


class CentralGitTests(unittest.TestCase):
    def setUp(self):
        print "setUp"
        getoutput("docker stop centralgit")
        getoutput("docker wait centralgit")
        for n in getoutput("docker ps --no-trunc -aq").split("\n"):
            getoutput("docker rm {}".format(n))
        getoutput("docker run --name centralgit -p 2222:22 -d centralgit/centralgit")

        # getoutput("ssh ssh://git@localhost:2222 cg delete test.git")
        # getoutput("ssh ssh://git@localhost:2222 cg create test.git")
        getoutput("rm -Rf _data*")
        getoutput("mkdir _data0")
        getoutput("mkdir _data1")

        # make sure the container is actually running
        while True:
            ok = getoutput("ssh -p 2222 git@localhost cg status")
            if "centralgit OK" in ok:
                break

    def tearDown(self):
        getoutput("ssh -p 2222 git@localhost cg log")
        getoutput("docker stop centralgit")
        getoutput("docker wait centralgit")
        for n in getoutput("docker ps --no-trunc -aq").split("\n"):
            getoutput("docker rm {}".format(n))

    def test_clone_all(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "echo bla > foo")
        in_data0("test", "git add foo")
        in_data0("test", "git commit -m add_foo")
        in_data0("test", 'git push')

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("bla", in_data1("test", "cat foo"))
        getoutput("tree")

    def test_clone_all_two_repos(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "echo bla > foo")
        in_data0("test", "git add foo")
        in_data0("test", "git commit -m add_foo")
        in_data0("test", 'git push')

        in_data0(".", "git clone ssh://git@localhost:2222/test2.git")
        in_data0("test2", "echo bla2 > foo")
        in_data0("test2", "git add foo")
        in_data0("test2", "git commit -m add_foo")
        in_data0("test2", 'git push')

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("bla", in_data1("test", "cat foo"))
        getoutput("tree")

        in_data1(".", "git clone ssh://git@localhost:2222/test2.git")
        self.assertEquals("bla2", in_data1("test2", "cat foo"))
        getoutput("tree")



    def test_clone_sub(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git push")

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

    def test_fetch_sub(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git push")

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data0("test", "echo sub_bla2 > sub/foo2")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo2")
        in_data0("test", "git push")

        in_data1("sub", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data1("sub", "cat foo2"))

    def test_commit_sub(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git push")

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data1("sub", "echo sub_bla2 > foo2")
        in_data1("sub", "git add foo2")
        in_data1("sub", "git commit -m add_foo2")
        in_data1("sub", "git push")

        in_data0("test", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data0("test", "cat sub/foo2"))

    def test_fetch_sub_not_master(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git checkout -b testbranch")
        in_data0("test", "git push origin testbranch:testbranch")

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        in_data1("test", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        in_data1("sub", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data0("test", "echo sub_bla2 > sub/foo2")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo2")
        in_data0("test", "git push origin testbranch:testbranch")

        in_data0("test", "git branch -u origin/testbranch testbranch")
        in_data1("sub", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data1("sub", "cat foo2"))

    def test_commit_sub_not_master(self):
        in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git checkout -b testbranch")
        in_data0("test", "git push origin testbranch:testbranch")

        in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        in_data1("test", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        in_data1("sub", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data1("sub", "echo sub_bla2 > foo2")
        in_data1("sub", "git add foo2")
        in_data1("sub", "git commit -m add_foo2")
        in_data1("sub", "git push origin testbranch:testbranch")


        in_data0("test", "git branch -u origin/testbranch testbranch")
        in_data0("test", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data0("test", "cat sub/foo2"))




if __name__ == "__main__":
    # getoutput("cp ~/.ssh/id_rsa.pub ../id_rsa.pub")
    status, output = getstatusoutput("cd .. && cargo test")
    print output
    if status == 0:
        print getoutput("cd .. && docker build -t centralgit/centralgit .")
        unittest.main()
