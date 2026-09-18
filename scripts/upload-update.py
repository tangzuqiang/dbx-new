import os
import posixpath
import sys

try:
    import paramiko
except ImportError:
    sys.exit("需要先安装：pip install paramiko")

if len(sys.argv) < 4:
    sys.exit("用法：upload-update.py <安装包路径> <latest.json路径> <远程文件名>")

host = os.environ["UPDATE_HOST"]
user = os.environ["UPDATE_USER"]
password = os.environ["UPDATE_PASS"]
remote_dir = os.environ["UPDATE_DIR"]
local_installer, local_manifest, remote_name = sys.argv[1], sys.argv[2], sys.argv[3]

if not os.path.isfile(local_installer):
    sys.exit(f"本地安装包不存在：{local_installer}")

version = os.environ.get("UPDATE_VERSION", "").strip()
notes = os.environ.get("UPDATE_NOTES", "").strip()
url = os.environ.get("UPDATE_URL", "").strip()
sha256 = os.environ.get("UPDATE_SHA256", "").strip()
if version and url and sha256:
    import json
    with open(local_manifest, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(
            {
                "version": version,
                "notes": notes or "功能更新",
                "url": url,
                "sha256": sha256,
                "silentArgs": "/S /UPDATE /R",
            },
            handle,
            ensure_ascii=False,
            separators=(",", ":"),
        )

if not os.path.isfile(local_manifest):
    sys.exit(f"本地清单不存在：{local_manifest}")

local_size = os.path.getsize(local_installer)
if local_size < 64:
    sys.exit(f"本地安装包过小：{local_size} 字节")

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect(host, username=user, password=password, timeout=30)
sftp = client.open_sftp()
try:
    try:
        sftp.mkdir(remote_dir)
    except OSError:
        pass
    remote_exe = posixpath.join(remote_dir, remote_name)
    remote_json = posixpath.join(remote_dir, "latest.json")
    print(f"上传安装包 {remote_name}（{local_size} 字节）…")
    sftp.put(local_installer, remote_exe)
    sftp.put(local_manifest, remote_json)
    remote_size = sftp.stat(remote_exe).st_size
    json_size = sftp.stat(remote_json).st_size
    names = sftp.listdir(remote_dir)
finally:
    sftp.close()
    client.close()

if remote_name not in names:
    sys.exit(f"上传后远程目录没有 {remote_name}，当前文件：{names}")
if remote_size != local_size:
    sys.exit(f"远程安装包大小不一致：本地 {local_size}，远程 {remote_size}")
if json_size < 8:
    sys.exit("远程 latest.json 异常")

print(f"已发布 {remote_name}（{remote_size} 字节）")
print("远程目录：", ", ".join(sorted(names)))
