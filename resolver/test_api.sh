#!/bin/bash

# Test script for Archypix Resolver

set -e

BASE_URL="http://localhost:8080"
MANAGED_URL="archypix.com"
ADMIN_TOKEN="${ADMIN_TOKEN:-change-me-in-production}"

echo "Testing Archypix Resolver API"
echo "=============================="
echo ""

# Test 1: Health check
echo "1. Health Check"
curl -s "${BASE_URL}/health" | jq .
echo ""
echo ""

# Test 2: Register a user
echo "2. Register user 'alice' -> 'https://backend1.archypix.com'"
curl -s -X POST "${BASE_URL}/api/update" \
  -H "Content-Type: application/json" \
  -d "{
    \"token\": \"${ADMIN_TOKEN}\",
    \"username\": \"alice\",
    \"backend_url\": \"https://backend1.archypix.com\"
  }" | jq .
echo ""
echo ""

# Test 3: Register another user
echo "3. Register user 'bob' -> 'https://backend2.archypix.com'"
curl -s -X POST "${BASE_URL}/api/update" \
  -H "Content-Type: application/json" \
  -d "{
    \"token\": \"${ADMIN_TOKEN}\",
    \"username\": \"bob\",
    \"backend_url\": \"https://backend2.archypix.com\"
  }" | jq .
echo ""
echo ""

# Test 4: WebFinger lookup for alice
echo "4. WebFinger lookup for 'alice'"
curl -s "${BASE_URL}/.well-known/webfinger?resource=acct:@alice:${MANAGED_URL}" | jq .
echo ""
echo ""

# Test 5: WebFinger lookup for bob
echo "5. WebFinger lookup for 'bob'"
curl -s "${BASE_URL}/.well-known/webfinger?resource=acct:@bob:${MANAGED_URL}" | jq .
echo ""
echo ""

# Test 6: WebFinger lookup for non-existent user
echo "6. WebFinger lookup for non-existent user 'charlie'"
curl -s "${BASE_URL}/.well-known/webfinger?resource=acct:@charlie:${MANAGED_URL}" | jq .
echo ""
echo ""

# Test 7: Update existing user
echo "7. Update 'alice' to 'https://backend3.archypix.com'"
curl -s -X POST "${BASE_URL}/api/update" \
  -H "Content-Type: application/json" \
  -d "{
    \"token\": \"${ADMIN_TOKEN}\",
    \"username\": \"alice\",
    \"backend_url\": \"https://backend3.archypix.com\"
  }" | jq .
echo ""
echo ""

# Test 8: Verify alice's updated mapping
echo "8. Verify updated mapping for 'alice'"
curl -s "${BASE_URL}/.well-known/webfinger?resource=acct:@alice:${MANAGED_URL}" | jq .
echo ""
echo ""

# Test 9: Unauthorized update attempt
echo "9. Test unauthorized update (wrong token)"
curl -s -X POST "${BASE_URL}/api/update" \
  -H "Content-Type: application/json" \
  -d "{
    \"token\": \"wrong-token\",
    \"username\": \"eve\",
    \"backend_url\": \"https://backend1.archypix.com\"
  }" | jq .
echo ""
echo ""

echo "=============================="
echo "All tests completed!"
