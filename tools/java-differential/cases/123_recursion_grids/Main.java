import java.util.ArrayList;

public class Main {
    static class Queens {
        private int n;
        private int[] cols;
        int solutions = 0;
        String first = "";

        Queens(int n) {
            this.n = n;
            this.cols = new int[n];
        }

        private boolean safe(int row, int col) {
            for (int r = 0; r < row; r++) {
                int c = cols[r];
                if (c == col || c - col == r - row || c - col == row - r) {
                    return false;
                }
            }
            return true;
        }

        void solve(int row) {
            if (row == n) {
                solutions++;
                if (first.equals("")) {
                    for (int r = 0; r < n; r++) {
                        first += cols[r];
                    }
                }
                return;
            }
            for (int col = 0; col < n; col++) {
                if (safe(row, col)) {
                    cols[row] = col;
                    solve(row + 1);
                }
            }
        }
    }

    static int fill(char[][] grid, int r, int c, char from, char to) {
        if (r < 0 || c < 0 || r >= grid.length || c >= grid[0].length) { return 0; }
        if (grid[r][c] != from) { return 0; }
        grid[r][c] = to;
        return 1 + fill(grid, r + 1, c, from, to) + fill(grid, r - 1, c, from, to)
            + fill(grid, r, c + 1, from, to) + fill(grid, r, c - 1, from, to);
    }

    static long paths(int r, int c, long[][] memo) {
        if (r == 0 || c == 0) { return 1; }
        if (memo[r][c] != 0) { return memo[r][c]; }
        long v = paths(r - 1, c, memo) + paths(r, c - 1, memo);
        memo[r][c] = v;
        return v;
    }

    static long ackermann(long m, long n) {
        if (m == 0) { return n + 1; }
        if (n == 0) { return ackermann(m - 1, 1); }
        return ackermann(m - 1, ackermann(m, n - 1));
    }

    static void permute(String prefix, String rest, ArrayList<String> out) {
        if (rest.length() == 0) {
            out.add(prefix);
            return;
        }
        for (int i = 0; i < rest.length(); i++) {
            String pick = rest.substring(i, i + 1);
            String remaining = rest.substring(0, i) + rest.substring(i + 1);
            permute(prefix + pick, remaining, out);
        }
    }

    static String classify(char cell, int neighbours) {
        return switch (neighbours) {
            case 0 -> cell == '#' ? "lonely wall" : "open field";
            case 1, 2 -> cell == '#' ? "thin wall" : "path";
            default -> cell == '#' ? "fortress" : "courtyard";
        };
    }

    public static void main(String[] args) {
        for (int n = 4; n <= 8; n++) {
            Queens q = new Queens(n);
            q.solve(0);
            System.out.println("queens " + n + ": " + q.solutions + " first " + q.first);
        }

        char[][] grid = {
            {'.', '.', '#', '.', '.'},
            {'.', '#', '#', '.', '#'},
            {'.', '.', '.', '#', '.'},
            {'#', '#', '.', '.', '.'}
        };
        System.out.println("filled " + fill(grid, 0, 0, '.', 'o'));
        for (char[] row : grid) {
            String line = "";
            for (char ch : row) {
                line += ch;
            }
            System.out.println(line);
        }

        long[][] memo = new long[17][17];
        System.out.println("paths 16x16 = " + paths(16, 16, memo));
        System.out.println("ackermann(2, 3) = " + ackermann(2, 3));
        System.out.println("ackermann(3, 3) = " + ackermann(3, 3));

        ArrayList<String> perms = new ArrayList<>();
        permute("", "abcd", perms);
        System.out.println(perms.size() + " perms, 7th " + perms.get(6) + ", last " + perms.get(perms.size() - 1));

        for (int r = 0; r < grid.length; r++) {
            int walls = 0;
            for (int c = 0; c < grid[r].length; c++) {
                if (grid[r][c] == '#') { walls++; }
            }
            System.out.println("row " + r + ": " + classify(grid[r][0], walls));
        }
    }
}
