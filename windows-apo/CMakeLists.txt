cmake_minimum_required(VERSION 3.20)
project(DivioraAPO LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(DivioraAPO SHARED
    src/DivioraAPO.cpp
    src/ClassFactory.cpp
    src/ApoState.cpp
    src/ApoProcess.cpp
    src/ControlService.cpp
)

target_include_directories(DivioraAPO PRIVATE include)
target_link_libraries(DivioraAPO PRIVATE Ole32 KsUser Advapi32)