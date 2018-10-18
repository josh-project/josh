FROM rust:1.23.0
RUN apt-get update
RUN apt-get install -y cmake
RUN apt-get install -y nginx
WORKDIR /usr/src/grib
COPY . .
RUN cargo install
COPY grib.conf /etc/nginx/sites-enabled/default
CMD /etc/init.d/nginx start && grib --local=/tmp/grib-scratch/ --remote=https://gerrit.int.esrlabs.com
