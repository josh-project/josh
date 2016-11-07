import unittest
import commands
from commands import getoutput
from commands import getstatusoutput

def getoutput(x):
    print "call:", x
    o = commands.getoutput(x)
    print "output:", o
    return o


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

    def in_data0(self,subdir, cmd):
        return getoutput("cd _data0/{} && {}".format(subdir,cmd))

    def in_data1(self,subdir, cmd):
        return getoutput("cd _data0/{} && {}".format(subdir,cmd))

    def tearDown(self):
        getoutput("ssh -p 2222 git@localhost cg log")
        getoutput("docker stop centralgit")
        getoutput("docker wait centralgit")
        for n in getoutput("docker ps --no-trunc -aq").split("\n"):
            getoutput("docker rm {}".format(n))

    def test_clone_all(self):
        self.in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        self.in_data0("test", "echo bla > foo")
        self.in_data0("test", "git add foo")
        self.in_data0("test", "git commit -m add_foo")
        self.in_data0("test", "git push")

        self.in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("bla", self.in_data1("test", "cat foo"))
        getoutput("tree")


    def test_clone_sub(self):
        self.in_data0(".", "git clone ssh://git@localhost:2222/test.git")
        self.in_data0("test", "mkdir sub && echo sub_bla > sub/foo")
        self.in_data0("test", "git add sub")
        self.in_data0("test", "git commit -m add_sub_foo")
        self.in_data0("test", "git push")

        self.in_data1(".", "git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("sub_bla", self.in_data1("test", "cat sub/foo"))

        self.in_data1(".", "git clone ssh://git@localhost:2222/test.git/sub")
        self.assertEquals("sub_bla", self.in_data1("sub", "cat foo"))
        getoutput("tree")


if __name__ == "__main__":
    getoutput("cp ~/.ssh/id_rsa.pub ../id_rsa.pub")
    status, output = getstatusoutput("cd .. && cargo test")
    print output
    if status == 0:
        print getoutput("cd .. && docker build -t centralgit/centralgit .")
        unittest.main()
