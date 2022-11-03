#!/usr/bin/bash
## Transcode an aedat4 file to ADΔER

## Example usage:
# ./evaluation/mmsys23/davis_to_adder/evaluate_davis_to_adder_realtime.sh /media/andrew/ExternalM2/DynamicVision ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation 40


DATASET_PATH=$1   # e.g., /media/andrew/ExternalM2/DynamicVision
FILELIST=$2   # e.g., ./evaluation/mmsys23/davis_to_adder/dataset/test_filelist.txt
DATA_LOG_PATH=$3  # e.g., /media/andrew/ExternalM2/10_26_22_davis_to_adder_evaluation
REF_TIME=1000000  # match the temporal granularity of the camera (microseconds)
DTM="$((1000000 * 4))"  # 4 seconds
TEMP_DIR=$4
echo "${DTM}"
mapfile -t filenames < "${FILELIST}"

#while IFS="\n" read FILENAME; do
for f in "${!filenames[@]}"; do
    FILENAME="${filenames[f]}"
    echo "${FILENAME}"
    if [ ! -d "${DATA_LOG_PATH}/${FILENAME}" ]; then # TODO: re-enable
#    if [ true ]; then
        mkdir "${DATA_LOG_PATH}/${FILENAME}"

            echo "${FILENAME}_${i}_${REF_TIME}"
            cargo run --bin davis_to_adder --release -- \
              --edi-args "
                                           args_filename = \"\"
                                           base_path = \"${DATASET_PATH}\"
                                           mode = \"file\"
                                           events_filename_0 = \"${FILENAME}\"
                                           events_filename_1 = \"\"
                                           start_c = 0.30344322344322345
                                           optimize_c = true
                                           optimize_controller = true
                                           deblur_only = false
                                           events_only = false
                                           simulate_packet_latency = true
                                           target_latency = 2000.0
                                           show_display = false
                                           show_blurred_display = false
                                           output_fps = 500
                                           write_video = false" \
                --args-filename "" \
                --output-events-filename "${TEMP_DIR}/tmp_events.adder" \
                --adder-c-thresh-pos 0 \
                --adder-c-thresh-neg 0 \
                --delta-t-max-multiplier 4.0 \
                --transcode-from "framed-davis" \
                --optimize-adder-controller \
                --write-out \
                --show-display \
                >> "${DATA_LOG_PATH}/${FILENAME}/${REF_TIME}.txt"
#                --show-display



            cargo run --release --bin adderinfo -- -i "${TEMP_DIR}/tmp_events.adder" -d >> "${DATA_LOG_PATH}/${FILENAME}/${REF_TIME}.txt"
#            cargo run --release --bin adder_to_dvs -- -i "${TEMP_DIR}/tmp_events.adder" \
#                --output-text "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}_dvs.txt" \
#                --output-video "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}_dvs.mp4" \
#                --fps 1000.0

#            rm -rf "${TEMP_DIR}/tmp_events.adder"   # Delete the events file
#            docker run -v ${DATASET_PATH}:/gt_vids -v "${TEMP_DIR}":/gen_vids gfdavila/easyvmaf -r "/gt_vids/${FILENAME}" -d /gen_vids/tmp.mp4 -sw 0.0 -ss 0 -endsync
#            rm -rf "${TEMP_DIR}/tmp.mp4"
#            mv "${TEMP_DIR}/tmp_vmaf.json" "${DATA_LOG_PATH}/${FILENAME}/${i}_${REF_TIME}_vmaf.json"
        sleep 5s
    fi
done

