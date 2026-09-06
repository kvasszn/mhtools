typedef unsigned char		byte;
typedef unsigned int		dword;
typedef long long			longlong;
typedef unsigned char		uchar;
typedef unsigned int		uint;
typedef unsigned long		ulong;
typedef unsigned long long	ulonglong;
typedef unsigned short		ushort;
typedef unsigned short		word;

typedef struct vec4 {
	float x;
	float y;
	float z;
	float w;
} Vec4;

typedef struct vec3 {
	float x;
	float y;
	float z;
} Vec3;

typedef struct vec2 {
	float x;
	float y;
} Vec2;

/*
mhgjp/afs_data/em33_amh.bin
Start: count=0x4, size=0x1e380 @ 0x0
    VersionOrSomething: count=0x1, size=0x10 @ 0xc
    Meshes: count=0x1, size=0x1c9fc @ 0x1c
        Mesh: count=0x9, size=0x1c9f0 @ 0x28
            IndexList: count=0x2, size=0x2904 @ 0x34
                Indices1: count=0x79, size=0xcdc @ 0x40
                Indices2: count=0x101, size=0x1c1c @ 0xd1c
            MaterialList: count=0xc, size=0x3c @ 0x2938
            MaterialIndex: count=0x17a, size=0x5f4 @ 0x2974
            Vertex: count=0x620, size=0x498c @ 0x2f68
            Normal: count=0x620, size=0x498c @ 0x78f4
            TexCoords: count=0x620, size=0x310c @ 0xc280
            Color: count=0x620, size=0x620c @ 0xf38c
            Weight: count=0x620, size=0x742c @ 0x15598
            Attribute: count=0x1, size=0x54 @ 0x1c9c4
    MaterialHead: count=0xc, size=0xccc @ 0x1ca18
        type=1: count=0x1, size=0x110 @ 0x1ca24
        type=1: count=0x1, size=0x110 @ 0x1cb34
        type=1: count=0x1, size=0x110 @ 0x1cc44
        type=1: count=0x1, size=0x110 @ 0x1cd54
        type=1: count=0x1, size=0x110 @ 0x1ce64
        type=1: count=0x1, size=0x110 @ 0x1cf74
        type=1: count=0x1, size=0x110 @ 0x1d084
        type=1: count=0x1, size=0x110 @ 0x1d194
        type=1: count=0x1, size=0x110 @ 0x1d2a4
        type=1: count=0x1, size=0x110 @ 0x1d3b4
        type=1: count=0x1, size=0x110 @ 0x1d4c4
        type=1: count=0x1, size=0x110 @ 0x1d5d4
    TextureHead: count=0xc, size=0xc9c @ 0x1d6e4
        type=0: count=0x1, size=0x10c @ 0x1d6f0
        type=0: count=0x1, size=0x10c @ 0x1d7fc
        type=0: count=0x1, size=0x10c @ 0x1d908
        type=0: count=0x1, size=0x10c @ 0x1da14
        type=0: count=0x1, size=0x10c @ 0x1db20
        type=0: count=0x1, size=0x10c @ 0x1dc2c
        type=0: count=0x1, size=0x10c @ 0x1dd38
        type=0: count=0x1, size=0x10c @ 0x1de44
        type=0: count=0x1, size=0x10c @ 0x1df50
        type=0: count=0x1, size=0x10c @ 0x1e05c
        type=0: count=0x1, size=0x10c @ 0x1e168
        type=0: count=0x1, size=0x10c @ 0x1e274
*/

typedef enum amo_type {
    IndexedSubData=-1,
    //Unknown=0, // Shouldn't ever happen if properly handling Materials and Textures
    Start=1,
    Meshes=2,
    IndicesData=3,
    Mesh=4,
    IndexList=5,
    MaterialIds=6,
    PositionData=7,
    NormalData=8,
    MaterialHead=9, // This skips the first 0xc bytes (looks like a head, but it's not)
    TextureHead=0xa,

    // These don't have children (leafs)
    ModelAttributeUnk0x10000=0x10000,
    VersionOrSomething=0x20000, // I think this is actually kinda called Meshes in code, miight be
                                // some flag
    Indices1=0x30000,
    Indices2=0x40000,
    MaterialList=0x50000,
    MaterialIndex=0x60000,
    Vertex=0x70000,
    Normal=0x80000,
    TexCoords=0xa0000,
    Color=0xb0000,
    Weight=0xc0000,
    UnknownModelSomething0xe0000=0xe0000,
    Attribute=0xf0000,
    MatrixList=0x100000
} AmoType;

typedef struct amo_head {
    AmoType type;
    uint count;
    uint size;
} AmoHead;

typedef struct amo_indices{
	uint count;
    int indices[1];
} AmoIndices;

typedef struct amo_index_list {
	amo_head head;
	union {
		AmoIndices indices1;
		AmoIndices indices2;
	} indices[1];
} AmoIndexList;

typedef struct amo_attribute {
    enum amo_type type;
	byte data[0x44]; //?
} AmoModelAttribute;

typedef struct weights {
	int count;
	struct joint {
		int id;
		float weight;
	} joints[1];
} Weights;

typedef struct amo_mesh_subdata {
	amo_head head;
	union {
		AmoIndexList index_list[1];
		int material_list[1];
		int material_index[1];
		int matrix_list[1];
		Vec3 vertices[1];
		Vec2 normals[1];
		Vec2 tex_coords[1];
		Vec4 colors[1];
		Weights weights[1];
	} data[1];
} AmoMeshSubData;

typedef struct amo_meshes {
	AmoHead head;
	AmoMeshSubData data[1];
} AmoMeshes;

typedef struct amo_texture_data {
    uint id;
    uint width;
    uint height;
	byte _pad0[0xf4];
} AmoTextureData;

typedef struct amo_texture_head {
	int type; // always zero in mh1/g
	uint count;
	uint size;
	AmoTextureData data[1];
} AmoTextureHead;

typedef struct amo_material_data {
    float ambient[4];
    float diffuse[4];
    float specular[4];
    float shininess;
    int illumination;
	int _pad0[0x32];
    int illum_tex_count;
} AmoMaterialData;

typedef enum material_data_type {
	MaterialData0 = 0,
	MaterialData1 = 1,
	MaterialData2 = 2,
	MaterialData5 = 5,
} MaterialDataType;

typedef struct amo_material_head {
	int type;
	int count;
	int size;
    AmoMaterialData data[1];
} AmoMaterialHead;

typedef struct amo {
	AmoHead head; // should equal Start (0x1)
	union {
		uint versionithink;
		AmoMeshes meshes;
		AmoMaterialHead materials;
		AmoTextureHead textures;
	} data[1];
} Amo;
