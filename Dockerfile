FROM fedora
RUN dnf -y update
RUN dnf -y install openssh-server
RUN dnf -y install passwd
RUN dnf -y install git
RUN dnf clean all

RUN ssh-keygen -t dsa -f /etc/ssh/ssh_host_dsa_key
RUN ssh-keygen -t rsa -f /etc/ssh/ssh_host_rsa_key

RUN adduser git

COPY id_rsa.pub /root/.ssh/authorized_keys
RUN mkdir /home/git/.ssh
RUN echo -n "command=\"/bin/centralgit-ssh --user testuser\" " >  /home/git/.ssh/authorized_keys
RUN cat /root/.ssh/authorized_keys >> /home/git/.ssh/authorized_keys

RUN chown -R root /root/.ssh
RUN chmod 700 /root/.ssh
RUN chmod 600 /root/.ssh/authorized_keys

RUN chown -R git /home/git/.ssh
RUN chmod 700 /home/git/.ssh
RUN chmod 600 /home/git/.ssh/authorized_keys

COPY target/debug/centralgit-ssh /bin/

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D"]
