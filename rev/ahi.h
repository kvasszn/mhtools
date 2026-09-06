typedef unsigned char		byte;
typedef unsigned int		dword;
typedef long long			longlong;
typedef unsigned char		uchar;
typedef unsigned int		uint;
typedef unsigned long		ulong;
typedef unsigned long long	ulonglong;
typedef unsigned short		ushort;
typedef unsigned short		word;

typedef enum {
	AhiFile = 0xc0000000
} FileType;

typedef struct {
	FileType type;
} FileHead;

typedef struct {
	uint unk0;
	uint unk1;
	uint unk2;
	uint unk3;
	uint unk4;
} AhiHeader;

typedef struct {}

typedef struct {
	FileHead head;
	AhiHeader header;
} Ahi;
