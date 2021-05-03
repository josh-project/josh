# usage: compare_screenshots new_dir old_dir

new_dir=$1
old_dir=$2

for file in $new_dir/*.png; do
    filename=$(basename "$file")
    sha_new=$(sha256sum "$file" | cut -d " " -f 1 )
    sha_old=$(sha256sum "${old_dir}/${filename}" | cut -d " " -f 1 )
    if [ "$sha_new" != "$sha_old" ]; then
        echo "${filename} has changed"
        cp "$file"  "${old_dir}/${filename}.err"
    fi
done

