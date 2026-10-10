idf_component_register(
    SRCS "main.cpp"
    INCLUDE_DIRS "."
    PRIV_REQUIRES app)

idf_component_set_property(${COMPONENT_NAME} WHOLE_ARCHIVE ON)
