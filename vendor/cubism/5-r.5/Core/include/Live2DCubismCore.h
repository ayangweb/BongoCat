/**
 * Copyright(c) Live2D Inc. All rights reserved.
 *
 * Use of this source code is governed by the Live2D Proprietary Software license
 * that can be found at https://www.live2d.com/eula/live2d-proprietary-software-license-agreement_en.html.
 */

#ifndef LIVE2D_CUBISM_CORE_H
#define LIVE2D_CUBISM_CORE_H

#if defined(__cplusplus)
extern "C"
{
#endif

/* ------- *
 * DEFINES *
 * ------- */

/** Core API attribute. */
#if !defined(csmApi)
#define csmApi
#endif

    /* ----- *
     * TYPES *
     * ----- */

    /** Cubism moc. */
    typedef struct csmMoc csmMoc;

    /** Cubism model. */
    typedef struct csmModel csmModel;

    /** Cubism version identifier. */
    typedef unsigned int csmVersion;

    /** Alignment constraints. */
    enum
    {
        /** Necessary alignment for mocs (in bytes). */
        csmAlignofMoc = 64,

        /** Necessary alignment for models (in bytes). */
        csmAlignofModel = 16
    };

    /** Bit masks for non-dynamic drawable flags. */
    enum
    {
        /** Additive blend mode mask. */
        csmBlendAdditive = 1 << 0,

        /** Multiplicative blend mode mask. */
        csmBlendMultiplicative = 1 << 1,

        /** Double-sidedness mask. */
        csmIsDoubleSided = 1 << 2,

        /** Clipping mask inversion mode mask. */
        csmIsInvertedMask = 1 << 3
    };

    /** Bit masks for dynamic drawable flags. */
    enum
    {
        /** Flag set when visible. */
        csmIsVisible = 1 << 0,
        /** Flag set when visibility did change. */
        csmVisibilityDidChange = 1 << 1,
        /** Flag set when opacity did change. */
        csmOpacityDidChange = 1 << 2,
        /** Flag set when draw order did change. */
        csmDrawOrderDidChange = 1 << 3,
        /** Flag set when render order did change. */
        csmRenderOrderDidChange = 1 << 4,
        /** Flag set when vertex positions did change. */
        csmVertexPositionsDidChange = 1 << 5,
        /** Flag set when blend color did change. */
        csmBlendColorDidChange = 1 << 6
    };

    /** Bitfield. */
    typedef unsigned char csmFlags;

    /** moc3 file format version. */
    enum
    {
        /** unknown */
        csmMocVersion_Unknown = 0,
        /** moc3 file version 3.0.00 - 3.2.07 */
        csmMocVersion_30 = 1,
        /** moc3 file version 3.3.00 - 3.3.03 */
        csmMocVersion_33 = 2,
        /** moc3 file version 4.0.00 - 4.1.05 */
        csmMocVersion_40 = 3,
        /** moc3 file version 4.2.00 - 4.2.04 */
        csmMocVersion_42 = 4,
        /** moc3 file version 5.0.00 - 5.2.03 */
        csmMocVersion_50 = 5,
        /** moc3 file version 5.3.00 - */
        csmMocVersion_53 = 6
    };

    /** moc3 version identifier. */
    typedef unsigned int csmMocVersion;

    /** Parameter types. */
    enum
    {
        /** Normal parameter. */
        csmParameterType_Normal = 0,

        /** Parameter for blend shape. */
        csmParameterType_BlendShape = 1
    };

    /** Color blend types. */
    enum
    {
        /** Normal blend. */
        csmColorBlendType_Normal = 0,
        /** Add blend. */
        csmColorBlendType_Add = 3,
        /** AddGlow blend. */
        csmColorBlendType_AddGlow = 4,
        /** Darken blend. */
        csmColorBlendType_Darken = 5,
        /** Multiply blend. */
        csmColorBlendType_Multiply = 6,
        /** ColorBurn blend. */
        csmColorBlendType_ColorBurn = 7,
        /** LinearBurn blend. */
        csmColorBlendType_LinearBurn = 8,
        /** Lighten blend. */
        csmColorBlendType_Lighten = 9,
        /** Screen blend. */
        csmColorBlendType_Screen = 10,
        /** ColorDodge blend. */
        csmColorBlendType_ColorDodge = 11,
        /** Overlay blend. */
        csmColorBlendType_Overlay = 12,
        /** SoftLight blend. */
        csmColorBlendType_SoftLight = 13,
        /** HardLight blend. */
        csmColorBlendType_HardLight = 14,
        /** LinearLight blend. */
        csmColorBlendType_LinearLight = 15,
        /** Hue blend. */
        csmColorBlendType_Hue = 16,
        /** Color blend. */
        csmColorBlendType_Color = 17,

        /** For version 5.2 and earlier. */

        /** Add compatible blend. */
        csmColorBlendType_AddCompatible = 1,
        /** Multiply compatible blend. */
        csmColorBlendType_MultiplyCompatible = 2
    };

    /** Alpha blend types. */
    enum
    {
        /** Over blend. */
        csmAlphaBlendType_Over = 0,
        /** Atop blend. */
        csmAlphaBlendType_Atop = 1,
        /** Out blend. */
        csmAlphaBlendType_Out = 2,
        /** ConjointOver blend. */
        csmAlphaBlendType_ConjointOver = 3,
        /** DisjointOver blend. */
        csmAlphaBlendType_DisjointOver = 4
    };

    /** Parameter type. */
    typedef int csmParameterType;

    /** 2 component vector. */
    typedef struct
    {
        /** First component. */
        float X;

        /** Second component. */
        float Y;
    } csmVector2;

    /** 4 component vector. */
    typedef struct
    {
        /** 1st component. */
        float X;

        /** 2nd component. */
        float Y;

        /** 3rd component. */
        float Z;

        /** 4th component. */
        float W;
    } csmVector4;

    /** Log handler.
     *
     * @param  message  Null-terminated string message to log.
     */
    typedef void (*csmLogFunction)(const char* message);

#if CSM_CORE_WIN32_DLL
#define csmCallingConvention __stdcall
#else
#define csmCallingConvention
#endif

    /* ------- *
     * VERSION *
     * ------- */

    /**
     * Queries Core version.
     *
     * @return  Core version.
     */
    csmApi csmVersion csmCallingConvention csmGetVersion();

    /**
     * Gets Moc file supported latest version.
     *
     * @return csmMocVersion (Moc file latest format version).
     */
    csmApi csmMocVersion csmCallingConvention csmGetLatestMocVersion();

    /**
     * Gets Moc file format version.
     *
     * @param  address  Address of moc.
     * @param  size     Size of moc (in bytes).
     *
     * @return csmMocVersion
     */
    csmApi csmMocVersion csmCallingConvention csmGetMocVersion(const void* address, const unsigned int size);

    /* ----------- *
     * CONSISTENCY *
     * ----------- */

    /**
     * Checks consistency of a moc.
     *
     * @param  address  Address of unrevived moc. The address must be aligned to 'csmAlignofMoc'.
     * @param  size     Size of moc (in bytes).
     *
     * @return  '1' if Moc is valid; '0' otherwise.
     */
    csmApi int csmCallingConvention csmHasMocConsistency(void* address, const unsigned int size);

    /* ------- *
     * LOGGING *
     * ------- */

    /**
     * Queries log handler.
     *
     * @return  Log handler.
     */
    csmApi csmLogFunction csmCallingConvention csmGetLogFunction();

    /**
     * Sets log handler.
     *
     * @param  handler  Handler to use.
     */
    csmApi void csmCallingConvention csmSetLogFunction(csmLogFunction handler);

    /* --- *
     * MOC *
     * --- */

    /**
     * Tries to revive a moc from bytes in place.
     *
     * @param  address  Address of unrevived moc. The address must be aligned to 'csmAlignofMoc'.
     * @param  size     Size of moc (in bytes).
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi csmMoc* csmCallingConvention csmReviveMocInPlace(void* address, const unsigned int size);

    /* ----- *
     * MODEL *
     * ----- */

    /**
     * Queries size of a model in bytes.
     *
     * @param  moc  Moc to query.
     *
     * @return  Valid size on success; '0' otherwise.
     */
    csmApi unsigned int csmCallingConvention csmGetSizeofModel(const csmMoc* moc);

    /**
     * Tries to instantiate a model in place.
     *
     * @param  moc      Source moc.
     * @param  address  Address to place instance at. Address must be aligned to 'csmAlignofModel'.
     * @param  size     Size of memory block for instance (in bytes).
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi csmModel* csmCallingConvention csmInitializeModelInPlace(const csmMoc* moc,
                                                                    void* address,
                                                                    const unsigned int size);

    /**
     * Updates a model.
     *
     * @param  model  Model to update.
     */
    csmApi void csmCallingConvention csmUpdateModel(csmModel* model);

    /**
     * Gets model draw orders.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetRenderOrders(const csmModel* model);

    /* ------ *
     * CANVAS *
     * ------ */

    /**
     * Reads info on a model canvas.
     *
     * @param  model              Model query.
     *
     * @param  outSizeInPixels    Canvas dimensions.
     * @param  outOriginInPixels  Origin of model on canvas.
     * @param  outPixelsPerUnit   Aspect used for scaling pixels to units.
     */
    csmApi void csmCallingConvention csmReadCanvasInfo(const csmModel* model,
                                                       csmVector2* outSizeInPixels,
                                                       csmVector2* outOriginInPixels,
                                                       float* outPixelsPerUnit);

    /* ---------- *
     * PARAMETERS *
     * ---------- */

    /**
     * Gets number of parameters.
     *
     * @param[in]  model  Model to query.
     *
     * @return  Valid count on success; '-1' otherwise.
     */
    csmApi int csmCallingConvention csmGetParameterCount(const csmModel* model);

    /**
     * Gets parameter IDs.
     * All IDs are null-terminated ANSI strings.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const char** csmCallingConvention csmGetParameterIds(const csmModel* model);


    /**
     * Gets parameter types.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmParameterType* csmCallingConvention csmGetParameterTypes(const csmModel* model);

    /**
     * Gets minimum parameter values.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float* csmCallingConvention csmGetParameterMinimumValues(const csmModel* model);

    /**
     * Gets maximum parameter values.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float* csmCallingConvention csmGetParameterMaximumValues(const csmModel* model);

    /**
     * Gets default parameter values.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float* csmCallingConvention csmGetParameterDefaultValues(const csmModel* model);

    /**
     * Gets read/write parameter values buffer.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi float* csmCallingConvention csmGetParameterValues(csmModel* model);

    /**
     * Gets Parameter Repeat informations.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetParameterRepeats(const csmModel* model);

    /**
     * Gets number of key values of each parameter.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetParameterKeyCounts(const csmModel* model);

    /**
     * Gets key values of each parameter.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float** csmCallingConvention csmGetParameterKeyValues(const csmModel* model);


    /* ----- *
     * PARTS *
     * ----- */

    /**
     * Gets number of parts.
     *
     * @param  model  Model to query.
     *
     * @return  Valid count on success; '-1' otherwise.
     */
    csmApi int csmCallingConvention csmGetPartCount(const csmModel* model);

    /**
     * Gets parts IDs.
     * All IDs are null-terminated ANSI strings.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const char** csmCallingConvention csmGetPartIds(const csmModel* model);

    /**
     * Gets read/write part opacities buffer.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi float* csmCallingConvention csmGetPartOpacities(csmModel* model);

    /**
     * Gets part's parent part indices.
     *
     * @param   model   Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetPartParentPartIndices(const csmModel* model);

    /**
     * Gets part's offscreen indices.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetPartOffscreenIndices(const csmModel* model);

    /* --------- *
     * DRAWABLES *
     * --------- */

    /**
     * Gets number of drawables.
     *
     * @param  model  Model to query.
     *
     * @return  Valid count on success; '-1' otherwise.
     */
    csmApi int csmCallingConvention csmGetDrawableCount(const csmModel* model);

    /**
     * Gets drawable IDs.
     * All IDs are null-terminated ANSI strings.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const char** csmCallingConvention csmGetDrawableIds(const csmModel* model);

    /**
     * Gets constant drawable flags.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmFlags* csmCallingConvention csmGetDrawableConstantFlags(const csmModel* model);

    /**
     * Gets dynamic drawable flags.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmFlags* csmCallingConvention csmGetDrawableDynamicFlags(const csmModel* model);

    /**
     * Gets drawable blend modes.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableBlendModes(const csmModel* model);

    /**
     * Gets drawable texture indices.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableTextureIndices(const csmModel* model);

    /**
     * Gets drawable draw orders.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableDrawOrders(const csmModel* model);

    /**
     * Gets drawable opacities.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float* csmCallingConvention csmGetDrawableOpacities(const csmModel* model);

    /**
     * Gets numbers of masks of each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableMaskCounts(const csmModel* model);

    /**
     * Gets mask indices of each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int** csmCallingConvention csmGetDrawableMasks(const csmModel* model);

    /**
     * Gets number of vertices of each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableVertexCounts(const csmModel* model);

    /**
     * Gets vertex position data of each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; a null pointer otherwise.
     */
    csmApi const csmVector2** csmCallingConvention csmGetDrawableVertexPositions(const csmModel* model);

    /**
     * Gets texture coordinate data of each drawables.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmVector2** csmCallingConvention csmGetDrawableVertexUvs(const csmModel* model);

    /**
     * Gets number of triangle indices for each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableIndexCounts(const csmModel* model);

    /**
     * Gets triangle index data for each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const unsigned short** csmCallingConvention csmGetDrawableIndices(const csmModel* model);

    /**
     * Gets multiply color data for each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmVector4* csmCallingConvention csmGetDrawableMultiplyColors(const csmModel* model);

    /**
     * Gets screen color data for each drawable.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmVector4* csmCallingConvention csmGetDrawableScreenColors(const csmModel* model);

    /**
     * Gets drawable's parent part indices.
     *
     * @param   model   Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetDrawableParentPartIndices(const csmModel* model);

    /**
     * Resets all dynamic drawable flags.
     *
     * @param  model  Model containing flags.
     */
    csmApi void csmCallingConvention csmResetDrawableDynamicFlags(csmModel* model);

    /* ---------- *
     * OFFSCREENS *
     * ---------- */

    /**
     * Gets number of offscreens.
     *
     * @param  model  Model to query.
     *
     * @return  Valid count on success; '-1' otherwise.
     */
    csmApi int csmCallingConvention csmGetOffscreenCount(const csmModel* model);

    /**
     * Gets offscreen blend modes.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetOffscreenBlendModes(const csmModel* model);

    /**
     * Gets offscreen opacities.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const float* csmCallingConvention csmGetOffscreenOpacities(const csmModel* model);

    /**
     * Gets offscreen owner indices.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetOffscreenOwnerIndices(const csmModel* model);

    /**
     * Gets multiply color data for each offscreen.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmVector4* csmCallingConvention csmGetOffscreenMultiplyColors(const csmModel* model);

    /**
     * Gets screen color data for each offscreen.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmVector4* csmCallingConvention csmGetOffscreenScreenColors(const csmModel* model);

    /**
     * Gets numbers of masks of each offscreen.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int* csmCallingConvention csmGetOffscreenMaskCounts(const csmModel* model);

    /**
     * Gets mask indices of each offscreen.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const int** csmCallingConvention csmGetOffscreenMasks(const csmModel* model);

    /**
     * Gets constant offscreen flags.
     *
     * @param  model  Model to query.
     *
     * @return  Valid pointer on success; '0' otherwise.
     */
    csmApi const csmFlags* csmCallingConvention csmGetOffscreenConstantFlags(const csmModel* model);

#if defined(__cplusplus)
}
#endif

#endif
