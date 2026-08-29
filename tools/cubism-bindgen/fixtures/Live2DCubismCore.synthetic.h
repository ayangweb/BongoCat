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

typedef struct csmMoc csmMoc;
typedef struct csmModel csmModel;
typedef void(CSM_CALL *csmLogFunction)(const char *message);

typedef enum csmMocVersion {
  csmMocVersion_Unknown = 0,
  csmMocVersion_50 = 5,
} csmMocVersion;

enum {
  csmAlignofMoc = 64,
  csmAlignofModel = 16,
};

csmVersion CSM_CALL csmGetVersion(void);
csmMocVersion CSM_CALL csmGetLatestMocVersion(void);
csmMocVersion CSM_CALL csmGetMocVersion(const void *mocBytes,
                                       csmSizeInt mocSize);
csmBool CSM_CALL csmHasMocConsistency(const void *mocBytes,
                                      csmSizeInt mocSize);
csmMoc *CSM_CALL csmReviveMocInPlace(void *mocBytes, csmSizeInt mocSize);
csmSizeInt CSM_CALL csmGetSizeofModel(const csmMoc *moc);
csmModel *CSM_CALL csmInitializeModelInPlace(const csmMoc *moc,
                                             void *modelMemory,
                                             csmSizeInt modelSize);
void CSM_CALL csmUpdateModel(csmModel *model);
void CSM_CALL csmReadCanvasInfo(const csmModel *model,
                                csmFloat32 *outSizeInPixels,
                                csmFloat32 *outOriginInPixels,
                                csmFloat32 *outPixelsPerUnit);
csmInt32 CSM_CALL csmGetParameterCount(const csmModel *model);
csmInt32 CSM_CALL csmGetPartCount(const csmModel *model);
csmInt32 CSM_CALL csmGetDrawableCount(const csmModel *model);
const csmFloat32 **CSM_CALL
csmGetDrawableVertexPositions(const csmModel *model);
const unsigned short **CSM_CALL csmGetDrawableIndices(const csmModel *model);
void CSM_CALL csmResetDrawableDynamicFlags(csmModel *model);
void CSM_CALL csmSetLogFunction(csmLogFunction handler);

typedef struct VendorInternalType VendorInternalType;
void vendorInternalFunction(VendorInternalType *value);

#endif
