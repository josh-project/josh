#!/bin/bash
kill $(cat ${CRAMTMP}/server_pid)
kill $(cat ${CRAMTMP}/proxy_pid)
