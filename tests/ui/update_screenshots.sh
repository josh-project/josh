screenshot_dir="$(dirname $0)/screenshots"
for err_file in $screenshot_dir/*.png.err; do
    file=${err_file%.err}
    mv $err_file  $file
done
