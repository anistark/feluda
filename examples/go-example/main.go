package main

import (
	"fmt"
	"github.com/gin-gonic/gin"
	"github.com/spf13/cobra"
	"go.uber.org/zap"
)

func main() {
	logger, _ := zap.NewProduction()
	defer logger.Sync()

	logger.Info("Go example with transient dependencies")
	fmt.Println("Go example with transient dependencies")

	// Example usage of packages with transient dependencies
	r := gin.Default()
	cmd := &cobra.Command{
		Use:   "example",
		Short: "Example command",
	}

	_ = r
	_ = cmd
}
