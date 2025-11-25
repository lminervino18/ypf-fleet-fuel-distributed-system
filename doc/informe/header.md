---
geometry: top=2.54cm, bottom=2.54cm, left=1.9cm, right=1.9cm
fontsize: 12pt
linestretch: 0.98
lang: es
header-includes:
  - \usepackage{float}
  - \usepackage{svg}
  - \usepackage{xcolor}
  - \usepackage{listings}
  - |
    \lstset{
      language=C,
      breaklines=true,
      breakatwhitespace=true,
      basicstyle=\ttfamily\small,
      columns=fullflexible,
      numbers=left,
      numberstyle=\tiny\color{gray},
      stepnumber=1,
      numbersep=8pt,
      keywordstyle=\color{blue}\bfseries,
      commentstyle=\color{gray},
      stringstyle=\color{red!70!black},
      showstringspaces=false
    }
---

\begin{titlepage}
    \centering
    \vspace*{5cm}

    \includegraphics[width=0.4\textwidth]{logo-fiuba.png}\\[1cm]

    {\Huge \textbf{Trabajo Práctico 2 - YPF Ruta}}\\[0.2cm]

    {\large \textbf{Programación Concurrente}}\\[0.8cm]

    \begin{tabular}{lll}
        108397 - & \textbf{Alejo Ordoñez} & \href{https://github.com/alejoordonez02}{github.com/alejoordonez02} \\[-2pt]
        105666 - & \textbf{Francisco Pereyra} & \href{https://github.com/fapereyra}{github.com/fapereyra} \\[-2pt]
        107863 - & \textbf{Lorenzo Minervino} & \href{https://github.com/lminervino18}{github.com/lminervino18} \\[-2pt]
        103376 - & \textbf{Alejandro Paff} & \href{https://github.com/AlePaff}{github.com/AlePaff} \\[-2pt]
    \end{tabular}

    \vfill

    {\large 2c2025}
\end{titlepage}

\newpage
\tableofcontents
