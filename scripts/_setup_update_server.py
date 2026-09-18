import os
import posixpath

import paramiko

host = os.environ["UPDATE_HOST"]
user = os.environ["UPDATE_USER"]
password = os.environ["UPDATE_PASS"]
local_manifest = os.environ["UPDATE_MANIFEST"]
remote_dir = os.environ.get("UPDATE_DIR", "/usr/share/nginx/html/dbx")

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect(host, username=user, password=password, timeout=20)


def run(cmd: str) -> str:
    _stdin, stdout, stderr = client.exec_command(cmd)
    out = stdout.read().decode("utf-8", "ignore")
    err = stderr.read().decode("utf-8", "ignore")
    return (out + err).strip()


print("UNAME", run("uname -a"))
print("WEBDIRS", run("ls -ld /var/www/html /usr/share/nginx/html /www/wwwroot 2>/dev/null"))
print("LISTEN80", run("ss -lntp | grep ':80 ' || true"))

sftp = client.open_sftp()
parts = remote_dir.strip("/").split("/")
cur = ""
for part in parts:
    cur = posixpath.join(cur, part) if cur else "/" + part
    try:
        sftp.mkdir(cur)
    except OSError:
        pass
sftp.put(local_manifest, posixpath.join(remote_dir, "latest.json"))
sftp.close()
print("WROTE", posixpath.join(remote_dir, "latest.json"))
print("LS", run(f"ls -l {remote_dir}"))
client.close()

