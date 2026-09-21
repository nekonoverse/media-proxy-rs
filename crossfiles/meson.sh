set -eu
source /app/crossfiles/amd64.sh
if [ ! -f "/app/crossfiles/cross.txt" ]; then
tee /app/crossfiles/cross.txt << EOS
[binaries]
c = '${CC}'
cpp = '${CXX}'
ar = '${AR}'

[host_machine]
system = 'linux'
cpu_family = '${CPU_FAMILY}'
cpu = '${CPU}'
endian = '${ENDIAN}'
EOS
fi
