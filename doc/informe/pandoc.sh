pandoc \
    header.md\
    informe.md \
    --pdf-engine=xelatex \
    --pdf-engine-opt=--shell-escape \
    -o informe.pdf
    # --number-sections \
