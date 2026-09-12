#ifndef LIVE2D_CUBISM_CORE_SYNTHETIC_H
#define LIVE2D_CUBISM_CORE_SYNTHETIC_H

#define CSM_CALL

typedef unsigned char csmByte;
typedef int csmBool;
typedef int csmInt32;
typedef unsigned int csmUint32;
typedef float csmFloat32;
typedef unsigned long long csmSizeInt;
typedef csmUint32 csmVersion;
typedef csmByte csmFlags;
typedef csmInt32 csmParameterType;

typedef struct csmVector2 {
  csmFloat32 X;
  csmFloat32 Y;
} csmVector2;

typedef struct csmVector4 {
  csmFloat32 X;
  csmFloat32 Y;
  csmFloat32 Z;
  csmFloat32 W;
} csmVector4;

typedef struct csmMoc csmMoc;
typedef struct csmModel csmModel;
typedef void(CSM_CALL *csmLogFunction)(const char *message);

enum {
  csmMocVersion_Unknown = 0,
  csmMocVersion_50 = 5,
  csmMocVersion_53 = 6,
};
typedef csmUint32 csmMocVersion;

enum {
  csmAlignofMoc = 64,
  csmAlignofModel = 16,
};

csmVersion CSM_CALL csmGetVersion(void);
csmMocVersion CSM_CALL csmGetLatestMocVersion(void);
csmMocVersion CSM_CALL csmGetMocVersion(const void *mocBytes,
                                       csmUint32 mocSize);
csmBool CSM_CALL csmHasMocConsistency(void *mocBytes,
                                      csmUint32 mocSize);
csmMoc *CSM_CALL csmReviveMocInPlace(void *mocBytes, csmUint32 mocSize);
csmUint32 CSM_CALL csmGetSizeofModel(const csmMoc *moc);
csmModel *CSM_CALL csmInitializeModelInPlace(const csmMoc *moc,
                                             void *modelMemory,
                                             csmUint32 modelSize);
void CSM_CALL csmUpdateModel(csmModel *model);
const csmInt32 *CSM_CALL csmGetRenderOrders(const csmModel *model);
void CSM_CALL csmReadCanvasInfo(const csmModel *model,
                                csmVector2 *outSizeInPixels,
                                csmVector2 *outOriginInPixels,
                                csmFloat32 *outPixelsPerUnit);
csmInt32 CSM_CALL csmGetParameterCount(const csmModel *model);
const char **CSM_CALL csmGetParameterIds(const csmModel *model);
const csmParameterType *CSM_CALL
csmGetParameterTypes(const csmModel *model);
const csmFloat32 *CSM_CALL
csmGetParameterMinimumValues(const csmModel *model);
const csmFloat32 *CSM_CALL
csmGetParameterMaximumValues(const csmModel *model);
const csmFloat32 *CSM_CALL
csmGetParameterDefaultValues(const csmModel *model);
csmFloat32 *CSM_CALL csmGetParameterValues(csmModel *model);
csmInt32 CSM_CALL csmGetPartCount(const csmModel *model);
const char **CSM_CALL csmGetPartIds(const csmModel *model);
csmFloat32 *CSM_CALL csmGetPartOpacities(csmModel *model);
const csmInt32 *CSM_CALL csmGetPartOffscreenIndices(const csmModel *model);
csmInt32 CSM_CALL csmGetDrawableCount(const csmModel *model);
const char **CSM_CALL csmGetDrawableIds(const csmModel *model);
const csmFlags *CSM_CALL csmGetDrawableConstantFlags(const csmModel *model);
const csmFlags *CSM_CALL csmGetDrawableDynamicFlags(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableBlendModes(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableTextureIndices(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableDrawOrders(const csmModel *model);
const csmFloat32 *CSM_CALL csmGetDrawableOpacities(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableMaskCounts(const csmModel *model);
const csmInt32 **CSM_CALL csmGetDrawableMasks(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableVertexCounts(const csmModel *model);
const csmVector2 **CSM_CALL
csmGetDrawableVertexPositions(const csmModel *model);
const csmVector2 **CSM_CALL csmGetDrawableVertexUvs(const csmModel *model);
const csmInt32 *CSM_CALL csmGetDrawableIndexCounts(const csmModel *model);
const unsigned short **CSM_CALL csmGetDrawableIndices(const csmModel *model);
const csmVector4 *CSM_CALL
csmGetDrawableMultiplyColors(const csmModel *model);
const csmVector4 *CSM_CALL csmGetDrawableScreenColors(const csmModel *model);
const csmInt32 *CSM_CALL
csmGetDrawableParentPartIndices(const csmModel *model);
void CSM_CALL csmResetDrawableDynamicFlags(csmModel *model);
csmInt32 CSM_CALL csmGetOffscreenCount(const csmModel *model);
const csmInt32 *CSM_CALL csmGetOffscreenBlendModes(const csmModel *model);
const csmFloat32 *CSM_CALL csmGetOffscreenOpacities(const csmModel *model);
const csmInt32 *CSM_CALL csmGetOffscreenOwnerIndices(const csmModel *model);
const csmVector4 *CSM_CALL csmGetOffscreenMultiplyColors(const csmModel *model);
const csmVector4 *CSM_CALL csmGetOffscreenScreenColors(const csmModel *model);
const csmInt32 *CSM_CALL csmGetOffscreenMaskCounts(const csmModel *model);
const csmInt32 **CSM_CALL csmGetOffscreenMasks(const csmModel *model);
const csmFlags *CSM_CALL csmGetOffscreenConstantFlags(const csmModel *model);
void CSM_CALL csmSetLogFunction(csmLogFunction handler);

typedef struct VendorInternalType VendorInternalType;
void vendorInternalFunction(VendorInternalType *value);

#endif
