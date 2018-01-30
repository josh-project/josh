import unittest
import commands
from commands import getoutput
from commands import getstatusoutput

import os
dir_path = os.path.dirname(os.path.realpath(__file__))

def getoutput(x):
    print "call:", x

    o = commands.getoutput(x)
    print "output:", o
    return o

def in_data0(subdir, cmd):
    return getoutput("cd _data0/{} && {}".format(subdir,cmd))

def in_data1(subdir, cmd):
    return getoutput("cd _data1/{} && {}".format(subdir,cmd))


class CentralGitTests(unittest.TestCase):
    def setUp(self):
        print "setUp"
        getoutput("ssh -p 29418 admin@localhost delete-project delete --yes-really-delete test");
        getoutput("ssh -p 29418 admin@localhost gerrit create-project --empty-commit test");
        getoutput("rm -Rf _data*")
        getoutput("mkdir _data0")
        getoutput("mkdir _data1")

    def test_clone_all(self):
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "echo bla > foo")
        in_data0("test", "git add foo")
        in_data0("test", "git commit -m add_foo")

        in_data0("test", 'git log')

        in_data0("test", 'git push')

        in_data1(".", "git clone http://localhost:8000/ test")
        self.assertEquals("bla", in_data1("test", "cat foo"))
        getoutput("tree")

    def test_clone_sub(self):
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git log")
        in_data0("test", "git push")

        in_data1(".", "git clone http://localhost:8000/ test")
        getoutput("tree")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))


        in_data1(".", "git clone http://localhost:8000/sub.git")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))

    def test_fetch_sub(self):
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git push")

        in_data1(".", "git clone http://localhost:8000/ test")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone http://localhost:8000/sub.git")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data0("test", "echo sub_bla2 > sub/foo2")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo2")
        in_data0("test", "git push")

        in_data1("sub", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data1("sub", "cat foo2"))

    def test_commit_sub(self):
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git push")

        in_data1(".", "git clone http://localhost:8000/ test")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone http://localhost:8000/sub.git")
        self.assertEquals("sub_bla", in_data1("sub", "cat foo"))
        getoutput("tree")

        in_data1("sub", "echo sub_bla2 > foo2")
        in_data1("sub", "git add foo2")
        in_data1("sub", "git commit -m add_foo2")
        in_data1("sub", "git push")

        in_data0("test", "git pull --rebase")
        self.assertEquals("sub_bla2", in_data0("test", "cat sub/foo2"))
        return

    def test_fetch_sub_not_master(self):
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git checkout -b testbranch")
        in_data0("test", "git push origin testbranch:testbranch")

        in_data1(".", "git clone http://localhost:8000/ test")
        in_data1("test", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone http://localhost:8000/sub.git")
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
        in_data0(".", "git clone http://localhost:8000/ test")
        in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        in_data0("test", "git add sub")
        in_data0("test", "git commit -m add_sub_foo")
        in_data0("test", "git checkout -b testbranch")
        in_data0("test", "git push origin testbranch:testbranch")

        in_data1(".", "git clone http://localhost:8000/ test")
        in_data1("test", "git checkout testbranch")
        self.assertEquals("sub_bla", in_data1("test", "cat sub/foo"))

        in_data1(".", "git clone http://localhost:8000/sub.git")
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
    #status, output = getstatusoutput("cd .. && cargo test")
    #print output
    #if status == 0:
    unittest.main()
