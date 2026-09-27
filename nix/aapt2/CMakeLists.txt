# aapt2 for a Linux host, from the Android sources, without Soong.
#
# The source lists are the ones the Android build files name for the host
# variant of each library (and the ones Debian compiles on Linux, which are
# the same); the layout under ${SRC} is what nix/aapt2.nix unpacks:
#
#   base/     frameworks/base at platform-tools-35.0.2 (tools/aapt2,
#             libs/androidfw, cmds/idmap2/libidmap2_policies)
#   core/     system/core at the same tag (libutils, libcutils, libsystem)
#   deb/      Debian's android-platform-tools 35.0.2 source: system/libbase,
#             system/logging, system/libziparchive, system/incremental_delivery
#             (map_ptr only), frameworks/native (headers), external/fmtlib
#
# protobuf, libpng, expat and zlib are the host's.
cmake_minimum_required(VERSION 3.20)
project(aapt2 CXX C)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
add_compile_options(-Wno-attributes -Wno-unused-parameter -Wno-missing-field-initializers -Wno-deprecated-declarations)
# Android's libc++ headers include <cstring> and <cstdint> transitively;
# glibc's libstdc++ does not, and androidfw and aapt2 rely on it (Debian
# patches the same headers in). Only those two: libbase's
# posix_strerror_r.cpp needs to be the first to include <string.h>.
set(PRELUDE "$<$<COMPILE_LANGUAGE:CXX>:SHELL:-include ${PROJECT_SOURCE_DIR}/prelude.h>")
add_compile_definitions(_FILE_OFFSET_BITS=64)

set(SRC ${PROJECT_SOURCE_DIR}/src)
set(BASE ${SRC}/base)
set(CORE ${SRC}/core)
set(DEB ${SRC}/deb)

set(INC_LIBBASE ${DEB}/system/libbase/include)
set(INC_LIBLOG ${DEB}/system/logging/liblog/include)
set(INC_CORE ${CORE}/include)
set(INC_CUTILS ${CORE}/libcutils/include)
set(INC_UTILS ${CORE}/libutils/include)
set(INC_SYSTEM ${CORE}/libsystem/include)
set(INC_ZIP ${DEB}/system/libziparchive/include)
set(INC_INCFS_UTIL ${DEB}/system/incremental_delivery/incfs/util/include)
set(INC_NATIVE ${DEB}/frameworks/native/include)
set(INC_FMT ${DEB}/external/fmtlib/include)
set(INC_ANDROIDFW ${BASE}/libs/androidfw/include ${BASE}/libs/androidfw/include_pathutils)

# ---- liblog: the host logger --------------------------------------------
add_library(log STATIC
  ${DEB}/system/logging/liblog/log_event_list.cpp
  ${DEB}/system/logging/liblog/log_event_write.cpp
  ${DEB}/system/logging/liblog/logger_name.cpp
  ${DEB}/system/logging/liblog/logger_read.cpp
  ${DEB}/system/logging/liblog/logger_write.cpp
  ${DEB}/system/logging/liblog/logprint.cpp
  ${DEB}/system/logging/liblog/properties.cpp
  ${DEB}/system/logging/liblog/event_tag_map.cpp)
target_compile_definitions(log PRIVATE LIBLOG_LOG_TAG=1006 SNET_EVENT_LOG_TAG=1397638484)
target_include_directories(log PUBLIC ${INC_LIBLOG} PRIVATE ${INC_CORE} ${INC_CUTILS} ${INC_LIBBASE})

# ---- libbase, with fmt compiled in --------------------------------------
add_library(base STATIC
  ${DEB}/system/libbase/abi_compatibility.cpp
  ${DEB}/system/libbase/chrono_utils.cpp
  ${DEB}/system/libbase/cmsg.cpp
  ${DEB}/system/libbase/file.cpp
  ${DEB}/system/libbase/hex.cpp
  ${DEB}/system/libbase/logging.cpp
  ${DEB}/system/libbase/mapped_file.cpp
  ${DEB}/system/libbase/parsebool.cpp
  ${DEB}/system/libbase/parsenetaddress.cpp
  ${DEB}/system/libbase/posix_strerror_r.cpp
  ${DEB}/system/libbase/process.cpp
  ${DEB}/system/libbase/properties.cpp
  ${DEB}/system/libbase/result.cpp
  ${DEB}/system/libbase/stringprintf.cpp
  ${DEB}/system/libbase/strings.cpp
  ${DEB}/system/libbase/threads.cpp
  ${DEB}/system/libbase/test_utils.cpp
  ${DEB}/system/libbase/errors_unix.cpp
  ${DEB}/external/fmtlib/src/format.cc)
target_include_directories(base PUBLIC ${INC_LIBBASE} ${INC_FMT} PRIVATE ${INC_CORE} ${INC_LIBLOG})
target_link_libraries(base PUBLIC log pthread)

# ---- libcutils ------------------------------------------------------------
add_library(cutils STATIC
  ${CORE}/libcutils/sockets.cpp
  ${CORE}/libcutils/socket_inaddr_any_server_unix.cpp
  ${CORE}/libcutils/socket_local_client_unix.cpp
  ${CORE}/libcutils/socket_local_server_unix.cpp
  ${CORE}/libcutils/socket_network_client_unix.cpp
  ${CORE}/libcutils/sockets_unix.cpp
  ${CORE}/libcutils/fs.cpp
  ${CORE}/libcutils/hashmap.cpp
  ${CORE}/libcutils/multiuser.cpp
  ${CORE}/libcutils/str_parms.cpp
  ${CORE}/libcutils/config_utils.cpp
  ${CORE}/libcutils/iosched_policy.cpp
  ${CORE}/libcutils/load_file.cpp
  ${CORE}/libcutils/native_handle.cpp
  ${CORE}/libcutils/properties.cpp
  ${CORE}/libcutils/record_stream.cpp
  ${CORE}/libcutils/strlcpy.c
  ${CORE}/libcutils/ashmem-host.cpp
  ${CORE}/libcutils/canned_fs_config.cpp
  ${CORE}/libcutils/fs_config.cpp
  ${CORE}/libcutils/trace-host.cpp)
target_include_directories(cutils PUBLIC ${INC_CUTILS} PRIVATE ${INC_CORE} ${INC_LIBBASE} ${INC_LIBLOG})
target_link_libraries(cutils PUBLIC base log)

# ---- libutils, with incfs's map_ptr as Debian and Soong both do -------------
add_library(utils STATIC
  ${CORE}/libutils/FileMap.cpp
  ${CORE}/libutils/JenkinsHash.cpp
  ${CORE}/libutils/LightRefBase.cpp
  ${CORE}/libutils/NativeHandle.cpp
  ${CORE}/libutils/Printer.cpp
  ${CORE}/libutils/StopWatch.cpp
  ${CORE}/libutils/SystemClock.cpp
  ${CORE}/libutils/Threads.cpp
  ${CORE}/libutils/Timers.cpp
  ${CORE}/libutils/Tokenizer.cpp
  ${CORE}/libutils/misc.cpp
  ${CORE}/libutils/Looper.cpp
  ${CORE}/libutils/binder/RefBase.cpp
  ${CORE}/libutils/binder/SharedBuffer.cpp
  ${CORE}/libutils/binder/String8.cpp
  ${CORE}/libutils/binder/String16.cpp
  ${CORE}/libutils/binder/StrongPointer.cpp
  ${CORE}/libutils/binder/Unicode.cpp
  ${CORE}/libutils/binder/VectorImpl.cpp
  ${DEB}/system/incremental_delivery/incfs/util/map_ptr.cpp)
target_include_directories(utils PUBLIC ${INC_UTILS} ${INC_SYSTEM} ${INC_INCFS_UTIL}
  PRIVATE ${INC_CORE} ${INC_CUTILS} ${CORE}/libprocessgroup/include ${CORE}/libvndksupport/include ${INC_LIBBASE} ${INC_LIBLOG})
target_link_libraries(utils PUBLIC cutils base log pthread)

# ---- libziparchive, incfs support off as on every host --------------------
add_library(ziparchive STATIC
  ${DEB}/system/libziparchive/zip_archive.cc
  ${DEB}/system/libziparchive/zip_archive_stream_entry.cc
  ${DEB}/system/libziparchive/zip_cd_entry_map.cc
  ${DEB}/system/libziparchive/zip_error.cpp
  ${DEB}/system/libziparchive/zip_writer.cc)
target_compile_definitions(ziparchive PRIVATE INCFS_SUPPORT_DISABLED=1 ZLIB_CONST)
target_include_directories(ziparchive PUBLIC ${INC_ZIP} PRIVATE ${DEB}/system/libziparchive/incfs_support/include ${INC_CORE} ${INC_LIBBASE} ${INC_LIBLOG})
target_link_libraries(ziparchive PUBLIC base log z)

# ---- libandroidfw, the host variant -----------------------------------------
add_library(androidfw STATIC
  ${BASE}/libs/androidfw/ApkAssets.cpp
  ${BASE}/libs/androidfw/ApkParsing.cpp
  ${BASE}/libs/androidfw/Asset.cpp
  ${BASE}/libs/androidfw/AssetDir.cpp
  ${BASE}/libs/androidfw/AssetManager.cpp
  ${BASE}/libs/androidfw/AssetManager2.cpp
  ${BASE}/libs/androidfw/AssetsProvider.cpp
  ${BASE}/libs/androidfw/AttributeResolution.cpp
  ${BASE}/libs/androidfw/BigBuffer.cpp
  ${BASE}/libs/androidfw/BigBufferStream.cpp
  ${BASE}/libs/androidfw/ChunkIterator.cpp
  ${BASE}/libs/androidfw/ConfigDescription.cpp
  ${BASE}/libs/androidfw/FileStream.cpp
  ${BASE}/libs/androidfw/Idmap.cpp
  ${BASE}/libs/androidfw/LoadedArsc.cpp
  ${BASE}/libs/androidfw/Locale.cpp
  ${BASE}/libs/androidfw/LocaleData.cpp
  ${BASE}/libs/androidfw/misc.cpp
  ${BASE}/libs/androidfw/NinePatch.cpp
  ${BASE}/libs/androidfw/ObbFile.cpp
  ${BASE}/libs/androidfw/PosixUtils.cpp
  ${BASE}/libs/androidfw/Png.cpp
  ${BASE}/libs/androidfw/PngChunkFilter.cpp
  ${BASE}/libs/androidfw/PngCrunch.cpp
  ${BASE}/libs/androidfw/ResourceTimer.cpp
  ${BASE}/libs/androidfw/ResourceTypes.cpp
  ${BASE}/libs/androidfw/ResourceUtils.cpp
  ${BASE}/libs/androidfw/StreamingZipInflater.cpp
  ${BASE}/libs/androidfw/StringPool.cpp
  ${BASE}/libs/androidfw/TypeWrappers.cpp
  ${BASE}/libs/androidfw/Util.cpp
  ${BASE}/libs/androidfw/ZipFileRO.cpp
  ${BASE}/libs/androidfw/ZipUtils.cpp
  ${BASE}/libs/androidfw/PathUtils.cpp)
target_compile_definitions(androidfw PRIVATE STATIC_ANDROIDFW_FOR_TOOLS)
target_compile_options(androidfw PRIVATE ${PRELUDE})
target_include_directories(androidfw PUBLIC ${INC_ANDROIDFW} ${INC_NATIVE} PRIVATE ${INC_CORE})
target_link_libraries(androidfw PUBLIC utils cutils ziparchive base log png z)

# ---- the protocol buffers, generated at build time ---------------------------
# The .proto files import each other as "frameworks/base/tools/aapt2/X.proto",
# so protoc is run from a root where that path exists.
set(PROTO_ROOT ${PROJECT_SOURCE_DIR}/proto-root)
set(GEN ${CMAKE_CURRENT_BINARY_DIR}/gen)
set(PROTOS ApkInfo Configuration Resources ResourceMetadata ResourcesInternal)
set(PB_SRCS)
foreach(p ${PROTOS})
  list(APPEND PB_SRCS ${GEN}/frameworks/base/tools/aapt2/${p}.pb.cc)
endforeach()
set(PROTO_FILES)
foreach(p ${PROTOS})
  list(APPEND PROTO_FILES ${PROTO_ROOT}/frameworks/base/tools/aapt2/${p}.proto)
endforeach()
add_custom_command(
  OUTPUT ${PB_SRCS}
  COMMAND ${CMAKE_COMMAND} -E make_directory ${GEN}
  COMMAND protoc --proto_path=${PROTO_ROOT} --cpp_out=${GEN} ${PROTO_FILES}
  DEPENDS ${PROTO_FILES}
  COMMENT "protoc: aapt2's protocol buffers")

# ---- libaapt2 and aapt2 ------------------------------------------------------
set(AAPT2 ${BASE}/tools/aapt2)
add_library(aapt2lib STATIC
  ${AAPT2}/compile/IdAssigner.cpp
  ${AAPT2}/compile/InlineXmlFormatParser.cpp
  ${AAPT2}/compile/PseudolocaleGenerator.cpp
  ${AAPT2}/compile/Pseudolocalizer.cpp
  ${AAPT2}/compile/XmlIdCollector.cpp
  ${AAPT2}/configuration/ConfigurationParser.cpp
  ${AAPT2}/dump/DumpManifest.cpp
  ${AAPT2}/filter/AbiFilter.cpp
  ${AAPT2}/filter/ConfigFilter.cpp
  ${AAPT2}/format/Archive.cpp
  ${AAPT2}/format/Container.cpp
  ${AAPT2}/format/binary/BinaryResourceParser.cpp
  ${AAPT2}/format/binary/ResChunkPullParser.cpp
  ${AAPT2}/format/binary/ResEntryWriter.cpp
  ${AAPT2}/format/binary/TableFlattener.cpp
  ${AAPT2}/format/binary/XmlFlattener.cpp
  ${AAPT2}/format/proto/ProtoDeserialize.cpp
  ${AAPT2}/format/proto/ProtoSerialize.cpp
  ${AAPT2}/io/File.cpp
  ${AAPT2}/io/FileSystem.cpp
  ${AAPT2}/io/StringStream.cpp
  ${AAPT2}/io/Util.cpp
  ${AAPT2}/io/ZipArchive.cpp
  ${AAPT2}/link/AutoVersioner.cpp
  ${AAPT2}/link/FeatureFlagsFilter.cpp
  ${AAPT2}/link/ManifestFixer.cpp
  ${AAPT2}/link/NoDefaultResourceRemover.cpp
  ${AAPT2}/link/PrivateAttributeMover.cpp
  ${AAPT2}/link/ReferenceLinker.cpp
  ${AAPT2}/link/ResourceExcluder.cpp
  ${AAPT2}/link/TableMerger.cpp
  ${AAPT2}/link/XmlCompatVersioner.cpp
  ${AAPT2}/link/XmlNamespaceRemover.cpp
  ${AAPT2}/link/XmlReferenceLinker.cpp
  ${AAPT2}/optimize/MultiApkGenerator.cpp
  ${AAPT2}/optimize/ResourceDeduper.cpp
  ${AAPT2}/optimize/ResourceFilter.cpp
  ${AAPT2}/optimize/Obfuscator.cpp
  ${AAPT2}/optimize/VersionCollapser.cpp
  ${AAPT2}/process/ProductFilter.cpp
  ${AAPT2}/process/SymbolTable.cpp
  ${AAPT2}/split/TableSplitter.cpp
  ${AAPT2}/text/Printer.cpp
  ${AAPT2}/text/Unicode.cpp
  ${AAPT2}/text/Utf8Iterator.cpp
  ${AAPT2}/util/Files.cpp
  ${AAPT2}/util/Util.cpp
  ${AAPT2}/Debug.cpp
  ${AAPT2}/DominatorTree.cpp
  ${AAPT2}/java/AnnotationProcessor.cpp
  ${AAPT2}/java/ClassDefinition.cpp
  ${AAPT2}/java/JavaClassGenerator.cpp
  ${AAPT2}/java/ManifestClassGenerator.cpp
  ${AAPT2}/java/ProguardRules.cpp
  ${AAPT2}/LoadedApk.cpp
  ${AAPT2}/Resource.cpp
  ${AAPT2}/ResourceParser.cpp
  ${AAPT2}/ResourceTable.cpp
  ${AAPT2}/ResourceUtils.cpp
  ${AAPT2}/ResourceValues.cpp
  ${AAPT2}/SdkConstants.cpp
  ${AAPT2}/trace/TraceBuffer.cpp
  ${AAPT2}/xml/XmlActionExecutor.cpp
  ${AAPT2}/xml/XmlDom.cpp
  ${AAPT2}/xml/XmlPullParser.cpp
  ${AAPT2}/xml/XmlUtil.cpp
  ${AAPT2}/ValueTransformer.cpp
  ${PROJECT_SOURCE_DIR}/build/version.cpp
  ${PB_SRCS})
set_target_properties(aapt2lib PROPERTIES CXX_STANDARD 23)
target_compile_options(aapt2lib PRIVATE -fno-rtti ${PRELUDE})
target_include_directories(aapt2lib PUBLIC
  ${AAPT2} ${GEN} ${GEN}/frameworks/base/tools/aapt2
  ${BASE}/cmds/idmap2/libidmap2_policies/include
  ${PROJECT_SOURCE_DIR})
target_link_libraries(aapt2lib PUBLIC androidfw utils cutils ziparchive base log protobuf expat png z pthread)

add_executable(aapt2
  ${AAPT2}/Main.cpp
  ${AAPT2}/cmd/ApkInfo.cpp
  ${AAPT2}/cmd/Command.cpp
  ${AAPT2}/cmd/Compile.cpp
  ${AAPT2}/cmd/Convert.cpp
  ${AAPT2}/cmd/Diff.cpp
  ${AAPT2}/cmd/Dump.cpp
  ${AAPT2}/cmd/Link.cpp
  ${AAPT2}/cmd/Optimize.cpp
  ${AAPT2}/cmd/Util.cpp)
set_target_properties(aapt2 PROPERTIES CXX_STANDARD 23)
target_compile_options(aapt2 PRIVATE -fno-rtti ${PRELUDE})
target_link_libraries(aapt2 PRIVATE aapt2lib)
install(TARGETS aapt2 RUNTIME DESTINATION bin)
