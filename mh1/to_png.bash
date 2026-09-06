path=$1
if [ -z "$1" ]; then
	echo "path is empty"
	exit 1
fi
out=${2:-"outputs"}
find $path -type f -name "*_tex.bin" | while read -r file; do
    rel_path="${file#$path}"
    out_file="$out/${rel_path%.bin}.png"
    mkdir -p "$(dirname "$out_file")"
	echo "decoding $file to $out_file"
    ./target/release/mh1tool --input "$file" --output "$out_file" > /dev/null
done
