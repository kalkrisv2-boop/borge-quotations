#!/bin/bash
# generate-icons.sh
# Phase 2.2 — Desktop icon generation for Tauri v2 application
# Usage: ./generate-icons.sh
# 
# This script:
# 1. Verifies app-icon.png exists in project root
# 2. Runs `npx tauri icon app-icon.png` to generate multi-resolution assets
# 3. Places generated assets in src-tauri/icons/
# 4. Does NOT modify bundle.active in tauri.conf.json (that's a Phase 5 decision)

set -e

# Color output helpers
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}=== Tauri Icon Generation (Phase 2.2) ===${NC}"

# Check if app-icon.png exists
if [ ! -f "app-icon.png" ]; then
    echo -e "${RED}ERROR: app-icon.png not found in project root${NC}"
    echo "Expected location: $(pwd)/app-icon.png"
    exit 1
fi

echo -e "${GREEN}✓ Found app-icon.png${NC}"

# Verify app-icon.png dimensions are reasonable for icon generation
FILE_SIZE=$(stat -f%z "app-icon.png" 2>/dev/null || stat -c%s "app-icon.png" 2>/dev/null)
FILE_SIZE_KB=$((FILE_SIZE / 1024))

if [ "$FILE_SIZE_KB" -lt 50 ] || [ "$FILE_SIZE_KB" -gt 5000 ]; then
    echo -e "${YELLOW}WARNING: app-icon.png is ${FILE_SIZE_KB}KB (expected 50–5000KB)${NC}"
    echo "Icon generation may produce low-quality results."
    read -p "Continue anyway? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Check if src-tauri/icons directory exists
if [ ! -d "src-tauri/icons" ]; then
    echo -e "${YELLOW}Creating src-tauri/icons directory${NC}"
    mkdir -p "src-tauri/icons"
fi

# Run Tauri icon generator
echo -e "${YELLOW}Generating Tauri icon assets...${NC}"

if ! npx tauri icon app-icon.png; then
    echo -e "${RED}ERROR: Icon generation failed${NC}"
    echo "Ensure app-icon.png is a valid PNG/ICO file with recommended dimensions >= 512×512 pixels"
    exit 1
fi

echo -e "${GREEN}✓ Icon generation complete${NC}"

# Verify generated files
echo -e "${YELLOW}Verifying generated assets...${NC}"

EXPECTED_FILES=(
    "src-tauri/icons/icon.png"
    "src-tauri/icons/icon.icns"
    "src-tauri/icons/icon.ico"
)

MISSING_FILES=0
for file in "${EXPECTED_FILES[@]}"; do
    if [ -f "$file" ]; then
        echo -e "${GREEN}✓ $file${NC}"
    else
        echo -e "${YELLOW}⚠ $file not generated (may be expected for your build target)${NC}"
        ((MISSING_FILES++))
    fi
done

if [ "$MISSING_FILES" -eq "${#EXPECTED_FILES[@]}" ]; then
    echo -e "${RED}ERROR: No icon files were generated${NC}"
    exit 1
fi

# Reminder about bundle.active
echo ""
echo -e "${YELLOW}=== IMPORTANT REMINDERS ===${NC}"
echo "1. Icon assets have been generated in src-tauri/icons/"
echo "2. Do NOT manually edit files in src-tauri/icons/"
echo "3. Do NOT change bundle.active in tauri.conf.json yet"
echo "   → bundle.active is toggled in Phase 5, not Phase 2.2"
echo "4. To rebuild icons in future: delete icons and re-run this script"

echo ""
echo -e "${GREEN}=== Icon generation complete ===${NC}"
