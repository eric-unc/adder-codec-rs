#!/usr/bin/bash
## This script transcodes the DAVIS dataset to ADΔER at various ADΔER contrast thresholds, which remain fixed during
## the course of the transcode.

## Results are output in the form of a text log
## containing the execution time and results of `adderinfo`, and a json file containing the results of VMAF perceptual
## quality analysis of the framed reconstructions.

## Uses a ramdisk to avoid writing tons of temporary data to disk
## Ex create a ramdisk mounting point:
# sudo mkdir /mnt/tmp
## Ex mount the ram disk with 20 GB of RAM
# sudo mount -t tmpfs -o size=20g tmpfs /mnt/tmp


 ./evaluate_davis_to_adder_realtime.sh \
    /media/andrew/ExternalM2/mmsys23_davis_dataset \
    ./dataset/dataset_filelist.txt \
    /home/andrew/Documents/11_3_davis_framed_adder_realtime_results \
    /mnt/tmp
