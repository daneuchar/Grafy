package main

import "github.com/gin-gonic/gin"

func getUser(c *gin.Context) {
	c.JSON(200, gin.H{"id": c.Param("id")})
}

func createUser(c *gin.Context) {
	c.JSON(201, gin.H{"created": true})
}

func main() {
	r := gin.Default()
	r.GET("/users/:id", getUser)
	r.POST("/users", createUser)
}
