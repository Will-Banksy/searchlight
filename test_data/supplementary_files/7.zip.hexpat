// Just a quick pattern file for specifically 7.zip, as the one shipped with ImHex fails for that ZIP (and others). As such, assumes only one file.

import std.io;

struct LocalFileHeader {
    u32 signature;
    u16 version;
    u16 flags;
    u16 compression;
    u16 modtime;
    u16 moddate;
    u32 crc;
    u32 compressed_size;
    u32 uncompressed_size;
    u16 file_name_len;
    u16 extra_field_len;
    char string[file_name_len];
    u8 extra_field[extra_field_len];
};

struct CentralDirHeader {
    u32 signature;
    u16 version;
    u16 version_needed;
    u16 flags;
    u16 compression;
    u16 modtime;
    u16 moddate;
    u32 crc;
    u32 compressed_size;
    u32 uncompressed_size;
    u16 file_name_len;
    u16 extra_field_len;
    u16 file_comment_len;
    u16 disk_start;
    u16 internal_attr;
    u32 external_attr;
    u32 local_header_offset;
    char file_name[file_name_len];
    u8 extra_field[extra_field_len];
    char file_comment[file_comment_len];
};

struct ZipFile {
    LocalFileHeader hdr_0;
    u8 data_0[hdr_0.compressed_size];
    CentralDirHeader cd_hdr_0;
};

ZipFile zip @ 0x00;