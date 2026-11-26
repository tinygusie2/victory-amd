# Starts the r13 build (this repo, branch main) with the settings measured on an RX 9060 XT.
# Put this script next to victory.exe (cargo build --release -p victory) and set your own dump folder below.
$d = $PSScriptRoot
$game = if ($env:VICTORY_GAME_DIR) { $env:VICTORY_GAME_DIR } else { Join-Path $d 'GTA6' }  # your own console dump
$env:VICTORY_GAME_DIR = $game
$env:VICTORY_CACHE_DIR = "$d\cache"; $env:VICTORY_DATA_DIR = "$d\data"
$env:VICTORY_SHADER_LOOP_LIMIT = '256'; $env:VICTORY_MAX_COMPUTE_KB = '48'
$env:VICTORY_LATE_WAIT = '3'; $env:VICTORY_WIDE_BACK = '1'; $env:VICTORY_CARD_BUFFERS_MIN_MB = '1'; $env:VICTORY_CARD_BUFFERS_MB = '1024'
$env:VICTORY_TEXTURE_BACK = '1'; $env:VICTORY_INDIRECT_GATHER = '1'; $env:VICTORY_JOIN_PARTS = '1'; $env:VICTORY_INDIRECT_WINDOW = '1'; $env:VICTORY_LEVELS_ON_DEMAND = '1'
$env:VICTORY_COPY_PLAIN = '1'; $env:VICTORY_REMAP_LEAN = '1'; $env:VICTORY_CARD_REHASH_RUNS = '1'
# Pipelines that crash AMD's shader compiler (RX 9060 XT, driver 32.0.32029.2010)
$env:VICTORY_SKIP_PIPELINES = '072f5fe96efd27e1,0f1395e5942ca3b2,1a1089a18f7ff5f8,1b20028fc5fae124,1c02f8131c508c75,26fbf87d883e1381,27ce027e7188bed3,36bcbae3c6ee20b3,37d324dbc6c86d16,38047b50e3443232,38e7dedf37c90130,3fa1fda35402c93f,501f0865fdf45dfd,50949ebfee9eb51d,5c10a56026ce13dd,5ddd2ca380fd74d8,695febfd5efced05,70b1e13de6df8585,72ee7db2a361ed65,7b956e1f6d56609f,7fb216a0c6dee3a9,810e4e3cf4237f07,82d469a7fec64cb4,842de0c32bc8dcc5,8a13d3da7f64b8d1,96744a8cd6c0588f,a2d880ad5dea1f45,a4df980a1c65fdbe,a62731e90739f195,b740639a48e688c6,c2fed64ee9b4bf23,c5fc2bc7f3d416f4,d2c52fcd936985b4,d5784d991f3164ab,dc6ef2687950428a,dd66eddf464f4bb1,e2b858e568731a51,e932a28baa58153e,f33e805895d6dd30,fc0dcb1f5312423f,fcd45c3d0020311b,eeed82b877b44b5f'
$env:VICTORY_DISPLAY_MODE = '2560x1440'  # the size of your monitor
# r13 speed switches (VICTORY_BIT_EXACT_VIEWS stays off: it turns lit skin black on AMD)
$env:VICTORY_HEAP_WORKER = '1'; $env:VICTORY_PARALLEL_REFRESH = '1'; $env:VICTORY_SETTLE_ROOM = '1'; $env:VICTORY_LOOKUP_CACHE = '1'
Set-Location $d
& "$d\victory.exe" run "$game\game_scarlett_master.exe" --execute *> "$d\start.log"
